<script setup lang="ts">
import { markdown, markdownLanguage } from '@codemirror/lang-markdown';
import {
  history,
  historyKeymap,
  defaultKeymap,
  indentMore,
  indentLess,
} from '@codemirror/commands';
import { HighlightStyle, syntaxHighlighting, syntaxTree } from '@codemirror/language';
import { EditorState, type Extension } from '@codemirror/state';
import type { EditorView as EditorViewType } from '@codemirror/view';
import { EditorView, keymap, placeholder as placeholderExt } from '@codemirror/view';
import { tags } from '@lezer/highlight';
import { onBeforeUnmount, onMounted, ref, useTemplateRef, watch } from 'vue';

/**
 * markdown 用の CodeMirror 6 エディタ。
 *
 * SSR しない: CodeMirror は DOM を要求するため、マウント後にクライアントで組み立てる。
 * 呼び出し側 (説明欄・作成ダイアログ) はいずれもユーザー操作で初めて描画される場所なので、
 * サーバ描画される経路は無い。器の div は常に出るため、レイアウトは前後で変わらない。
 *
 * v-model は「外 → 中」を state の差分適用で、「中 → 外」を updateListener で運ぶ。
 * 中で起きた変更を外から戻す経路 (v-model の往復) で選択位置が飛ばないよう、
 * 自分が emit した値と一致する watch は無視する。
 */
const props = withDefaults(
  defineProps<{
    /** 編集中の本文 */
    modelValue: string;
    /** 空のときに薄く出す案内 */
    placeholder?: string;
    /** 読み取り専用にする (保存中など) */
    disabled?: boolean;
    /** 器の最小高さ (Tailwind クラス) */
    minHeightClass?: string;
    /** スクリーンリーダー向けのラベル。role=textbox の名前になる */
    ariaLabel?: string;
  }>(),
  {
    placeholder: '',
    disabled: false,
    minHeightClass: 'min-h-28',
    ariaLabel: undefined,
  },
);

const emit = defineEmits<{
  'update:modelValue': [value: string];
  blur: [event: FocusEvent];
  keydown: [event: KeyboardEvent];
  /** Mod-Enter。字下げで Tab を使う文脈からキーボードだけで確定して抜けるための口 */
  submit: [];
}>();

const host = useTemplateRef<HTMLDivElement>('host');
const view = ref<EditorView | null>(null);
/** 直近に自分が emit した値。v-model の往復で state を作り直さないための照合用 */
let lastEmitted: string | null = null;
/**
 * 破棄中フラグ。EditorView.destroy() はフォーカス中の DOM を外すため blur が飛ぶ。
 * これを素通しすると、呼び出し側が「blur = 確定」にしている場合に
 * 編集の取り消し (Escape で編集器を閉じる) が確定として扱われ、
 * 取り消したはずの下書き — 空なら本文の消去 — が保存される。
 */
let destroying = false;

/**
 * markdown の見出し・強調・リンク等の着色。
 * 色はアプリのテーマ変数から取り、`.dark` の切り替えに CSS 側で追従させる
 * (KFM サイドカーがコードブロックの配色で採っているのと同じ方針)。
 */
const highlightStyle = HighlightStyle.define([
  { tag: tags.heading, color: 'var(--foreground)', fontWeight: '600' },
  { tag: tags.strong, color: 'var(--foreground)', fontWeight: '600' },
  { tag: tags.emphasis, fontStyle: 'italic' },
  { tag: tags.strikethrough, textDecoration: 'line-through' },
  { tag: tags.link, color: 'var(--primary)', textDecoration: 'underline' },
  { tag: tags.url, color: 'var(--primary)' },
  { tag: tags.monospace, color: 'var(--primary)' },
  { tag: tags.quote, color: 'var(--muted-foreground)' },
  { tag: tags.list, color: 'var(--muted-foreground)' },
  { tag: tags.processingInstruction, color: 'var(--muted-foreground)' },
  { tag: tags.contentSeparator, color: 'var(--muted-foreground)' },
]);

/** 器 (Tailwind) に配色と枠を任せ、CodeMirror 側は寸法と余白だけ持つ */
const theme = EditorView.theme({
  // 器 (min-h-*) いっぱいに広がるよう高さを引き取る。器は grid なので子は既定で伸びるが、
  // cm-editor 自身が内容高で止まらないように height を明示する
  '&': { height: '100%', backgroundColor: 'transparent', color: 'inherit', fontSize: 'inherit' },
  '&.cm-focused': { outline: 'none' },
  '.cm-content': {
    padding: '0.5rem 0.75rem',
    fontFamily: 'var(--font-mono, ui-monospace, monospace)',
    caretColor: 'var(--foreground)',
  },
  '.cm-line': { padding: '0' },
  '.cm-scroller': { lineHeight: '1.6', overflow: 'auto' },
  '.cm-placeholder': { color: 'var(--muted-foreground)' },
  '&.cm-editor .cm-selectionBackground, & .cm-selectionBackground': {
    backgroundColor: 'color-mix(in oklab, var(--primary) 25%, transparent)',
  },
  '&.cm-focused .cm-cursor': { borderLeftColor: 'var(--foreground)' },
});

/**
 * 字下げが意味を持つ文脈 (リスト項目・コードブロック・引用) にカーソルがあるか。
 * markdown の入れ子リストとコードの字下げは Tab で打てないと書けない一方、
 * 地の文で Tab を奪うと編集器から出られなくなるため、文脈で分ける。
 */
function inIndentableBlock(view: EditorViewType): boolean {
  const { state } = view;
  const tree = syntaxTree(state);
  return state.selection.ranges.some((range) => {
    for (let node = tree.resolveInner(range.head, -1); node; node = node.parent as never) {
      if (INDENTABLE_NODES.has(node.name)) return true;
      if (!node.parent) return false;
    }
    return false;
  });
}

const INDENTABLE_NODES = new Set([
  'ListItem',
  'BulletList',
  'OrderedList',
  'FencedCode',
  'CodeBlock',
  'CodeText',
  'Blockquote',
]);

/**
 * Tab の扱い。
 *
 * 常に字下げに使うと、blur で確定する inline 編集ではフォーカスが編集器から
 * 出られず確定もできなくなる。逆に常にフォーカス移動にすると、入れ子リストと
 * コードブロックの字下げが打てない。そこで
 *
 * - 複数行にまたがる選択がある、または字下げが意味を持つ文脈にいる → 字下げ
 * - それ以外 (地の文) → 何もせず既定のフォーカス移動に任せる
 *
 * 字下げ側に入った状態から抜ける手段として Mod-Enter (確定) を用意する。
 */
function tabIndents(view: EditorViewType): boolean {
  const spansLines = view.state.selection.ranges.some(
    (range) =>
      !range.empty &&
      view.state.doc.lineAt(range.from).number !== view.state.doc.lineAt(range.to).number,
  );
  return spansLines || inIndentableBlock(view);
}

/**
 * 差し替えが要る設定 (disabled / placeholder) は Compartment ではなく
 * state の作り直しで扱う。編集中に切り替わるのは保存中の一瞬だけで、
 * その間の履歴やカーソルは捨ててよいため、機構を増やさない。
 */
function extensions(): Extension[] {
  return [
    history(),
    // Escape は呼び出し側 (編集の取り消し) に渡すため CodeMirror の既定に載せない
    keymap.of([
      // 既定より先に置く (先に登録した束が優先される)
      {
        key: 'Tab',
        run: (view) => (tabIndents(view) ? indentMore(view) : false),
        shift: (view) => (tabIndents(view) ? indentLess(view) : false),
      },
      {
        key: 'Mod-Enter',
        run: () => {
          emit('submit');
          return true;
        },
      },
      ...defaultKeymap,
      ...historyKeymap,
    ]),
    // codeLanguages は渡さない: @codemirror/language-data は言語ごとの文法を
    // 全部引き連れてきてクライアントのチャンクが 100 個以上増える。説明欄は
    // markdown の構造が見えれば足り、コードの着色は表示側 (starry-night) が持つ
    markdown({ base: markdownLanguage }),
    syntaxHighlighting(highlightStyle),
    EditorView.lineWrapping,
    theme,
    EditorState.readOnly.of(props.disabled),
    EditorView.editable.of(!props.disabled),
    ...(props.placeholder ? [placeholderExt(props.placeholder)] : []),
    EditorView.contentAttributes.of({
      ...(props.ariaLabel ? { 'aria-label': props.ariaLabel } : {}),
      // 呼び出し側のテストと自動化がテキストエリアと同じ手掛かりで掴めるようにする
      'data-markdown-editor-input': '',
    }),
    EditorView.domEventHandlers({
      blur: (event) => {
        // 破棄に伴う blur は利用者の操作ではないので伝えない (上の destroying の説明)
        if (destroying) return false;
        emit('blur', event as FocusEvent);
        return false;
      },
      keydown: (event) => {
        emit('keydown', event);
        return false;
      },
    }),
    EditorView.updateListener.of((update) => {
      if (!update.docChanged) return;
      const value = update.state.doc.toString();
      lastEmitted = value;
      emit('update:modelValue', value);
    }),
  ];
}

onMounted(() => {
  if (!host.value) return;
  view.value = new EditorView({
    state: EditorState.create({ doc: props.modelValue, extensions: extensions() }),
    parent: host.value,
  });
});

onBeforeUnmount(() => {
  destroying = true;
  view.value?.destroy();
  view.value = null;
});

watch(
  () => props.modelValue,
  (next) => {
    const current = view.value;
    if (!current) return;
    // 自分が emit した値が戻ってきただけなら触らない (カーソルが末尾へ飛ぶのを防ぐ)
    if (next === lastEmitted) return;
    if (next === current.state.doc.toString()) return;
    current.dispatch({
      changes: { from: 0, to: current.state.doc.length, insert: next },
    });
  },
);

// disabled / placeholder の変更は state ごと作り直す (上のコメントの理由)
watch(
  () => [props.disabled, props.placeholder],
  () => {
    const current = view.value;
    if (!current) return;
    current.setState(EditorState.create({ doc: props.modelValue, extensions: extensions() }));
  },
);

/** 呼び出し側が編集開始時にフォーカスを移すために使う */
function focus() {
  view.value?.focus();
}

defineExpose({ focus });
</script>

<template>
  <div
    ref="host"
    data-markdown-editor
    class="border-input focus-within:border-ring focus-within:ring-ring/50 dark:bg-input/30 grid w-full overflow-hidden rounded-md border bg-transparent text-base shadow-xs transition-[color,box-shadow] focus-within:ring-[3px] md:text-sm"
    :class="[minHeightClass, disabled ? 'cursor-not-allowed opacity-50' : '']"
  />
</template>
