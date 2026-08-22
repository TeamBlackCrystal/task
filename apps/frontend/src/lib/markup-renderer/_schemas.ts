/*
 * 本番 renderer が注入する sanitize スキーマ一覧の単一ソース。
 * composition root (index.ts) の一部だが、index.ts はモジュール副作用で
 * createRenderer が走るため、スキーマ一覧だけ要る消費側 (story の攻撃 probe 等) が
 * 副作用なしで import できるようここへ分離する。コア実装 (_sanitize) はこれを
 * import しない (プラグイン注入の向きは composition root → コアのまま)。
 *
 * 二重管理を禁じる理由は実例がある: 本番が着色と mermaid のスキーマを足した一方、
 * story の probe は GFM と alerts の二つのままで、probe が本番より狭い許可集合を
 * 検査していた。ここを単一ソースにすれば、プラグインを足した時点で両方が動く。
 */
import { starryNightSanitizeSchema } from '@/lib/rehype-starry-night/schema';
import { gfmSanitizeSchema } from '@/lib/remark-gfm';
import { kfmMermaidSanitizeSchema } from '@/lib/remark-kfm-mermaid';
import { koyoriAlertsSanitizeSchema } from '@/lib/remark-koyori-alerts';
import type { SanitizeSchema } from './_sanitize';

/** 本番と story probe が共用する sanitize スキーマ一覧 (二重管理禁止) */
export const kfmSanitizeSchemas: readonly SanitizeSchema[] = [
  gfmSanitizeSchema,
  koyoriAlertsSanitizeSchema,
  starryNightSanitizeSchema,
  kfmMermaidSanitizeSchema,
];
