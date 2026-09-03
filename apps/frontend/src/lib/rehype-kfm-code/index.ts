/**
 * rehype-kfm-code — KFM 拡張: コードブロックを `<kfm-code>` で包み、帯 (言語名・
 * ファイル名) の data 属性と行番号用の行分割を SSR HTML へ焼く (Issue #683 第一段)。
 *
 * 層の位置は rehype 層の starry-night の「後ろ」(composition root の rehypePlugins 順):
 * - 行分割は着色済みの span (pl-*) を行ごとに切る仕事のため、着色より後ろでないと
 *   成立しない。starry-night は `<code class="language-*">` の子を置換するだけで
 *   wrapper を出さない契約 (rehype-starry-night/index.ts) ゆえ、後ろから包んで安全。
 * - mermaid フェンスは remark 層 (remark-kfm-mermaid) が hName を据えて pre>code を
 *   出さないため、本プラグインが誤って包む経路は構造的に無い (試験で固定)。
 *
 * emit の契約 (markup-renderer の規約):
 * - inline style は一切出さない (FORBID_ATTR: ['style'] と一枚岩)。新属性は
 *   data-lang / data-title のみで、schema.ts の SanitizeSchema と同時に増減する。
 * - SSR で焼くのは構造と data 属性のみ。複写ボタンは client の custom element
 *   (element.ts) が connectedCallback で足す — SSR HTML には入れない (sanitize の
 *   許可集合を button へ広げずに済む)。
 * - options は意図して受け取らない。_renderer.ts の processor fingerprint は plugin の
 *   関数名しか見ないため、closure に閉じた options はキャッシュキーに現れず、他構成の
 *   HTML を返し得る (rehype-starry-night と同じ理由)。
 *
 * 情報文字列の解釈 (data-title):
 * - ```ts title="src/foo.ts" — mdast の code.meta を mdast-util-to-hast の code handler が
 *   hast の code.data.meta へ写す (実物確認)。ここから title="…" だけを読む。
 * - ```ts:src/foo.ts — micromark は `ts:src/foo.ts` 丸ごとを lang とするため、
 *   language-* class の値を最初の `:` で lang と title に割る。この形の class
 *   (`language-ts:src/foo.ts`) は sanitize の language-* 許可字種 (: と / を含まない) の
 *   外なので出力には残らず、starry-night も言語として解釈しない (着色されないのは
 *   GitHub が : 形式を解釈しないのと同等の割り切り)。
 * - 両方あれば title="…" (明示の方) が勝つ。情報文字列の他の語は無視する。
 */
import type { Element, ElementContent, Root } from 'hast';
import { visit } from 'unist-util-visit';
import { KFM_CODE_TAG } from './_tag';

export { KFM_CODE_TAG } from './_tag';
export { kfmCodeSanitizeSchema } from './schema';

/** 行番号 1 行ぶんの器 (schema.ts の classTokens と同時に増減する) */
export const KFM_CODE_LINE_CLASS = 'pl-line' as const;

const LANGUAGE_CLASS_PREFIX = 'language-';
/** 情報文字列 (code.data.meta) から title="…" だけを読む。他の語は無視する */
const META_TITLE_RE = /(?:^|\s)title="([^"]*)"/;

function readLanguageClass(code: Element): string | undefined {
  const className = code.properties.className;
  if (!Array.isArray(className)) return undefined;
  const token = className.find(
    (value): value is string =>
      typeof value === 'string' && value.startsWith(LANGUAGE_CLASS_PREFIX),
  );
  return token?.slice(LANGUAGE_CLASS_PREFIX.length);
}

function readMetaTitle(code: Element): string | undefined {
  const meta = (code.data as { meta?: unknown } | undefined)?.meta;
  if (typeof meta !== 'string') return undefined;
  return META_TITLE_RE.exec(meta)?.[1];
}

/**
 * 着色済みの子ノード列を改行で行ごとに切る。行を跨ぐ span (starry-night は複数行
 * トークンを 1 つの pl-* span で出すことがある) は、行ごとに同じ properties の
 * span へ分けて着色を保つ。
 */
function splitIntoLines(nodes: readonly ElementContent[]): ElementContent[][] {
  const lines: ElementContent[][] = [[]];
  const append = (node: ElementContent): void => {
    lines[lines.length - 1]?.push(node);
  };
  for (const node of nodes) {
    if (node.type === 'text') {
      node.value.split('\n').forEach((part, index) => {
        if (index > 0) lines.push([]);
        if (part.length > 0) append({ type: 'text', value: part });
      });
    } else if (node.type === 'element') {
      splitIntoLines(node.children).forEach((chunk, index) => {
        if (index > 0) lines.push([]);
        if (chunk.length > 0) append({ ...node, children: chunk });
      });
    } else {
      append(node);
    }
  }
  return lines;
}

/**
 * code の子を行ごとに `<span class="pl-line">` で包む。
 * - 各行は行末の改行文字を span の中に持つ (display: block でも白空間 pre の最終改行は
 *   余分な行を作らず、複写 (textContent) には改行がそのまま残る)。
 * - mdast-util-to-hast の code handler は値の末尾に改行を 1 つ足すため、分割で生じる
 *   末尾の空 chunk は「余分な空行」として捨てる (中間の空行は行番号を振って残す)。
 */
function wrapLines(code: Element): void {
  const lines = splitIntoLines(code.children);
  if (lines.length > 1 && lines[lines.length - 1]?.length === 0) {
    lines.pop();
  }
  code.children = lines.map((children) => ({
    type: 'element',
    tagName: 'span',
    properties: { className: [KFM_CODE_LINE_CLASS] },
    children: [...children, { type: 'text', value: '\n' }],
  }));
}

export function rehypeKfmCode() {
  return function rehypeKfmCodeTransform(tree: Root): void {
    visit(tree, 'element', (node, index, parent) => {
      if (node.tagName !== 'pre' || parent === undefined || index === undefined) return;
      // markdown のコードフェンス由来は pre>code (単一子) のみ。それ以外の pre は
      // 本プラグインの対象外 (kfm-mermaid は remark 層で置換済みでここへ来ない)
      if (node.children.length !== 1) return;
      const code = node.children[0];
      if (code?.type !== 'element' || code.tagName !== 'code') return;

      const rawLanguage = readLanguageClass(code);
      const metaTitle = readMetaTitle(code);
      const separatorIndex = rawLanguage?.indexOf(':') ?? -1;
      const language =
        rawLanguage !== undefined && separatorIndex >= 0
          ? rawLanguage.slice(0, separatorIndex)
          : rawLanguage;
      const pathTitle =
        rawLanguage !== undefined && separatorIndex >= 0
          ? rawLanguage.slice(separatorIndex + 1)
          : undefined;
      const title = metaTitle ?? pathTitle;

      wrapLines(code);

      const wrapper: Element = {
        type: 'element',
        tagName: KFM_CODE_TAG,
        properties: {
          ...(language !== undefined && language.length > 0 ? { dataLang: language } : {}),
          ...(title !== undefined && title.length > 0 ? { dataTitle: title } : {}),
        },
        children: [node],
      };
      parent.children[index] = wrapper;
    });
  };
}
