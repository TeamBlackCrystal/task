/**
 * markup-renderer — KFM (Koyori Flavored Markdown) コア。
 * Phase 1 実体は github profile (= 複製レンダラ): GFM ＋ GitHub alerts ＋ 安全 core。
 *
 * SSR / Hydration 契約:
 * - サーバ生成 HTML を唯一の入力とする。ページの +data.ts で
 *   `descriptionHtml: await renderDescription(text)` を実行して pageContext.data に載せ、
 *   コンポーネントは `<div v-html="descriptionHtml" />` で受けるだけにする。
 * - 同一ページに複数の KFM 断片 (タスク本文＋コメント等) を並べる場合は、断片ごとに
 *   決定的な scope を渡す: `renderDescription(text, { scope: `comment-${id}` })`。
 *   脚注 id (user-content-<scope>-fn-*) の 1 ページ内衝突を避けるため。random でなく
 *   決定的なのは同一入力→同一 HTML (L1 キャッシュ・SSR/CSR 同一性) を保つため。
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
// client registry (registerKfmCustomElements と関連型) は意図的に root から再エクスポート
// しない。root は import しただけで下の createRenderer がモジュール副作用で走り、KFM 一式
// が client バンドルへ載る (root 経由 import で +417.5 KB raw、_client-registry 直接 import
// 化で 205.52 kB → 0.62 kB を実測)。client 専用 entry は
// `@/lib/markup-renderer/_client-registry` から直接 import すること。再エクスポートを
// 戻す変更は kfm-client-registry テスト (root 再エクスポート禁止) が機構として弾く。
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
  // config の既定 profile を描画既定へ実際に接続する (contentConfig はキャッシュキー用の
  // 不透明値でしかないため、ここで渡さない限り defaultProfile は描画に効かない)
  defaultProfile: contentConfig.defaultProfile,
});
