/**
 * KFM の GFM 層 — remark-gfm の薄いラッパ。
 * 消費側は remark-gfm を直接 import せず本モジュールを経由する
 * (将来 @koyori-app/remark-gfm へ切り出すときの seam)。
 */
export { default as remarkGfm } from 'remark-gfm';

/**
 * remark-gfm ＋ remark-rehype (GitHub 互換出力) が emit する固定 class トークン。
 * markup-renderer のコアはプラグインを import しない規約のため、composition root
 * (markup-renderer/index.ts) がこの export を createRenderer({ sanitizeSchemas }) へ渡し、
 * emit と sanitize の許可集合を単一ソース化する。
 */
export const gfmSanitizeSchema = {
  classTokens: [
    // タスクリスト
    'contains-task-list',
    'task-list-item',
    // 脚注 (mdast-util-to-hast の GitHub 互換 footer)
    'footnotes',
    'sr-only',
    'data-footnote-backref',
  ],
  classPatterns: [
    // コードフェンスの言語クラス。mdast-util-to-hast は info string をそのまま
    // language-<lang> にするため、実在する言語表記 (C++ / c# / TS / JSON /
    // objective-c) を通す字種が要る。大文字・+・#・.・_ のどれかを落とすと
    // starry-night が言語を認識できず着色されない (kfm-renderer テストで固定)。
    /^language-[A-Za-z0-9+#._-]+$/,
  ],
} as const;
