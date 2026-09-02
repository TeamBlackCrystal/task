import { defineComponent, h } from 'vue';

/**
 * 単体テスト用の MarkdownEditor の代役。
 *
 * 実物は CodeMirror で、happy-dom では起動できない (MutationObserver の実装差で
 * focus() や再描画が落ちる)。呼び出し側の単体テストが見たいのは
 * 「下書きが編集器へ渡り、変更・blur・keydown が返ってくる」という配線なので、
 * 同じ v-model / emit の面を持つ textarea に差し替える。
 * 実物の挙動は実ブラウザで動く story が見る。
 *
 * 使い方 (import より前に置くこと。vi.mock は巻き上げられる):
 *   vi.mock('@/components/markdown/MarkdownEditor.vue', async () => ({
 *     default: (await import('@/components/markdown/__tests__/markdown-editor-stub')).default,
 *   }));
 */
export default defineComponent({
  name: 'MarkdownEditorStub',
  props: { modelValue: { type: String, default: '' } },
  emits: ['update:modelValue', 'blur', 'keydown'],
  setup(props, { emit, attrs, expose }) {
    expose({ focus: () => {} });
    return () =>
      h('textarea', {
        ...attrs,
        value: props.modelValue,
        onInput: (event: Event) =>
          emit('update:modelValue', (event.target as HTMLTextAreaElement).value),
        onBlur: (event: FocusEvent) => emit('blur', event),
        onKeydown: (event: KeyboardEvent) => emit('keydown', event),
      });
  },
});
