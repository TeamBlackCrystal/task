/**
 * markup-renderer — KFM (Koyori Flavored Markdown) コア。
 * Phase 1 実体は github profile (= 複製レンダラ): GFM ＋ GitHub alerts ＋ 安全 core。
 *
 * SSR / Hydration 契約:
 * - サーバ生成 HTML を唯一の入力とする。ページの +data.ts で
 *   `descriptionHtml: await renderDescription(text)` を実行して pageContext.data に載せ、
 *   コンポーネントは `<div v-html="descriptionHtml" />` で受けるだけにする。
 * - クライアントは再パース・再サニタイズしない (DOMPurify はサーバで一度だけ)。
 * - alert の見た目は消費側で `@/lib/remark-koyori-alerts/style.css` を明示 import する
 *   (サイドカー方式)。
 *
 * renderDescription はモジュールトップレベル singleton = プロセス全体 (SSR では全
 * リクエスト・全 tenant) で共有される。L1 キャッシュが full-text キーであることが
 * この共有の安全条件 (_cache.ts 参照)。
 *
 * 本ファイルは composition root であり、プラグイン (remark 層 ＋ sanitize スキーマ) を
 * コアへ注入する。コア実装 (_renderer / _sanitize / _cache) はプラグインを import しない。
 */
import { gfmSanitizeSchema, remarkGfm } from '@/lib/remark-gfm';
import { koyoriAlertsSanitizeSchema, remarkKoyoriAlerts } from '@/lib/remark-koyori-alerts';
import { resolveContentConfig } from './_config';
import { createRenderer } from './_renderer';

export type { KfmContentConfig } from './_config';
export { resolveContentConfig } from './_config';
export type {
  CreateRendererOptions,
  KfmProfile,
  ProfileDefinition,
  RenderDescription,
  RenderOptions,
} from './_renderer';
export { createRenderer } from './_renderer';
export type {
  KfmCustomElementDefinition,
  RegisterKfmCustomElementsResult,
} from './_client-registry';
export { registerKfmCustomElements } from './_client-registry';
export type { SanitizeSchema } from './_sanitize';

/** Phase 1: system 層の上書きなし = コード既定 (github profile) */
const contentConfig = resolveContentConfig();

export const renderDescription = createRenderer({
  profiles: {
    // github profile = 共有 core そのもの (GFM ＋ alerts ＋ sanitize ＋ cache ＋ SSR 契約)
    github: { remarkPlugins: [remarkGfm, remarkKoyoriAlerts] },
    // Phase 2 seam: kfm profile はここへ remark 層を足す (コアは不変)。
  },
  sanitizeSchemas: [gfmSanitizeSchema, koyoriAlertsSanitizeSchema],
  contentConfig,
});
