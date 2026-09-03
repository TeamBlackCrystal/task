/**
 * rehype-kfm-code が emit するタグ・属性・class の許可宣言。composition root が
 * createRenderer へ渡して sanitize registry と単一ソース化する (_schemas.ts 参照)。
 * - 属性は SSR で焼く data-lang / data-title のみ。複写状態 (data-kfm-code-copy) は
 *   client が立てる値でありサーバ生成 HTML に現れてはならないため宣言しない
 *   (kfm-mermaid の data-kfm-mermaid と同じ整理)。
 * - pl-line は行番号用の行 span。starry-night の /^pl-[a-z0-9]+$/ パターンにも
 *   一致するが、本プラグインの emit は本プラグインが宣言する (他 schema への相乗りは
 *   相手の削除で黙って壊れる)。
 */
import type { SanitizeSchema } from '../markup-renderer/_sanitize';
import { KFM_CODE_TAG } from './_tag';

export const kfmCodeSanitizeSchema = {
  tags: [KFM_CODE_TAG],
  attrs: { [KFM_CODE_TAG]: ['data-lang', 'data-title'] },
  classTokens: ['pl-line'],
} as const satisfies SanitizeSchema;
