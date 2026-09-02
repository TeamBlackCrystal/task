// @vitest-environment node
import { describe, expect, it } from 'vitest';
import { createSSRApp, h } from 'vue';
import { renderToString } from 'vue/server-renderer';
import MarkdownEditor from '../MarkdownEditor.vue';

/*
 * CodeMirror は DOM が無いと動かない。MarkdownEditor は onMounted まで
 * EditorView を作らない約束で、この試験がその約束を node 環境で機械照合する。
 * 壊れると SSR (vike) 側が document is not defined で落ち、
 * 説明欄を持つページ全体が 500 になる。
 */
describe('MarkdownEditor の SSR', () => {
  it('DOM 無しで描画でき、出るのは器だけ (CodeMirror は組み立てない)', async () => {
    const html = await renderToString(
      createSSRApp({
        render: () => h(MarkdownEditor, { modelValue: '# 見出し', ariaLabel: '説明' }),
      }),
    );

    expect(html).toContain('data-markdown-editor');
    // マウント後にしか生えない CodeMirror の内部要素が SSR 出力に無いこと
    expect(html).not.toContain('cm-editor');
    expect(html).not.toContain('cm-content');
    // 本文を素のまま吐いていない (SSR では中身を持たない器だけを出す)
    expect(html).not.toContain('# 見出し');
  });
});
