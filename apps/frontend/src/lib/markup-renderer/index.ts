/**
 * markup-renderer — KFM (Koyori Flavored Markdown) コア。
 * Phase 1 実体は github profile (= 複製レンダラ):
 * GFM ＋ GitHub alerts ＋ コードブロック着色 (starry-night) ＋ 安全 core。
 *
 * SSR / Hydration 契約:
 * - サーバ生成 HTML を唯一の入力とする。ページの +data.ts で
 *   `descriptionHtml: await renderDescription(text)` を実行して pageContext.data に載せ、
 *   コンポーネントは `<div v-html="descriptionHtml" />` で受けるだけにする。
 * - 同一ページに複数の KFM 断片 (タスク本文＋コメント等) を並べる場合は、断片ごとに
 *   決定的な scope を渡す: `renderDescription(text, { scope: `comment-${id}` })`。
 *   scope が prefix を付けるのは脚注の fn-* / fnref-* 系 id
 *   (user-content-<scope>-fn-* / -fnref-*) に限る。mdast-util-to-hast が固定で出す
 *   footnote-label (id と aria-describedby) は clobberPrefix を経由しないため
 *   ここでは衝突が残り、その穴は #588 の rehype 層が塞ぐ。random でなく
 *   決定的なのは同一入力→同一 HTML (L1 キャッシュ・SSR/CSR 同一性) を保つため。
 * - クライアントは再パース・再サニタイズしない (DOMPurify はサーバで一度だけ)。
 * - alert の見た目は消費側で `@/lib/remark-koyori-alerts/style.css` を明示 import する
 *   (サイドカー方式。alerts はレンダラが emit する .kfm-alert 等を CSS が直接指すため
 *   import だけで当たる)。GFM 要素 (リスト/blockquote/リンク) は
 *   `@/lib/remark-gfm/style.css` の明示 import に加え、v-html する器へ
 *   class="kfm-content" (@/lib/remark-gfm/content-class.ts) を付ける (Tailwind
 *   preflight 対策)。GFM 出力は素の ul/blockquote/a で掴む class が無いため、
 *   器 scope が無いと GFM CSS は一行も当たらない。
 * - コードブロック着色の見た目も同方式: 消費側で `@/lib/rehype-starry-night/style.css` を
 *   明示 import する (実体は @wooorm/starry-night の light/dark 両対応シート)。
 *
 * renderDescription はモジュールトップレベル singleton = プロセス全体 (SSR では全
 * リクエスト・全 tenant) で共有される。L1 キャッシュが full-text キーであることが
 * この共有の安全条件 (_cache.ts 参照)。
 *
 * 本ファイルは composition root であり、プラグイン (remark 層 ＋ sanitize スキーマ) を
 * コアへ注入する。コア実装 (_renderer / _sanitize / _cache) はプラグインを import しない。
 */
import { rehypeStarryNight, starryNightSanitizeSchema } from '@/lib/rehype-starry-night';
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
    // github profile = 共有 core そのもの (GFM ＋ alerts ＋ 着色 ＋ sanitize ＋ cache ＋ SSR 契約)
    github: {
      remarkPlugins: [remarkGfm, remarkKoyoriAlerts],
      // GitHub 同様のコードブロック着色。rehype 層 (remark-rehype の後段) に挿す。
      rehypePlugins: [rehypeStarryNight],
    },
    // Phase 2 seam: kfm profile はここへ remark / rehype 層を足す (コアは不変)。
  },
  sanitizeSchemas: [gfmSanitizeSchema, koyoriAlertsSanitizeSchema, starryNightSanitizeSchema],
  contentConfig,
  // config の既定 profile を描画既定へ実際に接続する (contentConfig はキャッシュキー用の
  // 不透明値でしかないため、ここで渡さない限り defaultProfile は描画に効かない)
  defaultProfile: contentConfig.defaultProfile,
});
