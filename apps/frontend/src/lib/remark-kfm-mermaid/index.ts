/**
 * remark-kfm-mermaid — KFM 拡張: mermaid フェンス (```mermaid) を `<kfm-mermaid>` へ変換する。
 *
 * 捕捉は remark 層 (mdast の code ノード) に限定する:
 * - remark-rehype より前に hName を据えるため、rehype 層 (rehype-starry-night や
 *   cmd_672 の language-* 拡張) は mermaid フェンスを pre>code として見ることがなく、
 *   層間の順序争いが構造的に起きない。
 * - サーバは図を焼かない。SSR 出力は「不活性タグ ＋ light DOM のソーステキスト」のみで、
 *   SVG 化は client の custom element (element.ts) が行う (Bun SSR に jsdom/headless を
 *   持ち込まない方針)。JS 無効環境ではソーステキストがそのまま見えるのがフォールバック。
 *
 * 出力は data.hName / hChildren の型付き emit のみ (生 html ノード・inline style なし)。
 * ソーステキストは rehype-stringify がエスケープし、client は textContent で復元する。
 */
import type { ElementContent, Properties } from 'hast';
import type { Root } from 'mdast';
import { visit } from 'unist-util-visit';
import type { SanitizeSchema } from '../markup-renderer/_sanitize';
import { KFM_MERMAID_TAG } from './_tag';

export { KFM_MERMAID_TAG } from './_tag';

// mdast-util-to-hast (remark-rehype 内部) と同一の Data 拡張 (remark-koyori-alerts と同型)。
declare module 'mdast' {
  interface Data {
    hName?: string | undefined;
    hProperties?: Properties | undefined;
    hChildren?: ElementContent[] | undefined;
  }
}

export function remarkKfmMermaid() {
  return (tree: Root): void => {
    visit(tree, 'code', (node, index, parent) => {
      // GitHub 同様に言語名は case-insensitive (```Mermaid も図として扱う)
      if ((node.lang ?? '').toLowerCase() !== 'mermaid') return;
      if (parent === undefined || index === undefined) return;
      // code ノードに hName を据えるだけでは不十分: mdast-util-to-hast の code handler は
      // applyData を内側 code 要素にのみ適用し、外側 <pre> ラッパと language-* class が
      // 残ってしまう (実測)。ノードごと置換して素の <kfm-mermaid>ソース</kfm-mermaid> を出す。
      parent.children[index] = {
        type: 'paragraph',
        data: {
          hName: KFM_MERMAID_TAG,
          // ソーステキストのみを light DOM に残す (JS 無効時のフォールバック表示を兼ねる)
          hChildren: [{ type: 'text', value: node.value }],
        },
        children: [],
      };
    });
  };
}

/**
 * 本プラグインが emit するタグの許可宣言。composition root が createRenderer へ渡して
 * sanitize registry と単一ソース化する。属性は SSR 段では一切許可しない —
 * data-kfm-mermaid はレンダリング結果として client が立てる状態であり、
 * サーバ生成 HTML に現れてはならない (remark-rehype が生 HTML を落とすため、
 * sanitizer まで届かない)。
 */
export const kfmMermaidSanitizeSchema = {
  tags: [KFM_MERMAID_TAG],
} as const satisfies SanitizeSchema;
