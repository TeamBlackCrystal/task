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
    // コードフェンスの言語クラス
    /^language-[a-z0-9-]+$/,
  ],
} as const;
