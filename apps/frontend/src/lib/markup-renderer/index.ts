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
 *   clobberPrefix が脚注の fn-* / fnref-* 系 id
 *   (user-content-<scope>-fn-* / -fnref-*) を scope 化し、続く core の rehype scope 層が
 *   clobberPrefix を経由しない footnote-label (id と aria-describedby) も scope 化する。
 *   これにより断片間で全脚注 id の衝突を防ぐ。random でなく
 *   決定的なのは同一入力→同一 HTML (L1 キャッシュ・SSR/CSR 同一性) を保つため。
 * - クライアントは再パース・再サニタイズしない (DOMPurify はサーバで一度だけ)。
 * - alert の見た目は消費側で `@/lib/remark-koyori-alerts/style.css` を明示 import する
 *   (サイドカー方式。alerts はレンダラが emit する .kfm-alert 等を CSS が直接指すため
 *   import だけで当たる)。GFM 要素 (リスト/blockquote/リンク) は
 *   `@/lib/remark-gfm/style.css` の明示 import に加え、v-html する器へ
 *   class="kfm-content" (@/lib/remark-gfm/content-class.ts) を付ける (Tailwind
 *   preflight 対策)。GFM 出力は素の ul/blockquote/a で掴む class が無いため、
 *   器 scope が無いと GFM CSS は一行も当たらない。
 * - mermaid のフォールバック表示は消費側で
 *   `@/lib/remark-kfm-mermaid/style.css` を明示 import する。
 * - コードブロック着色の見た目も同方式: 消費側で `@/lib/rehype-starry-night/style.css` を
 *   明示 import する (実体は @wooorm/starry-night の light シート固定 ＋ .dark ブリッジ。
 *   選定理由は同 CSS のコメントを参照)。
 *
 * renderDescription はモジュールトップレベル singleton = プロセス全体 (SSR では全
 * リクエスト・全 tenant) で共有される。L1 キャッシュが full-text キーであることが
 * この共有の安全条件 (_cache.ts 参照)。
 *
 * 本ファイルは composition root であり、プラグイン (remark 層 ＋ sanitize スキーマ) を
 * コアへ注入する。コア実装 (_renderer / _sanitize / _cache) はプラグインを import しない。
 */
import { starryNightSanitizeSchema } from '@/lib/rehype-starry-night/schema';
import { gfmSanitizeSchema, remarkGfm } from '@/lib/remark-gfm';
import { kfmMermaidSanitizeSchema, remarkKfmMermaid } from '@/lib/remark-kfm-mermaid';
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

type StarryNightTransformer = ReturnType<
  ReturnType<(typeof import('@/lib/rehype-starry-night'))['createRehypeStarryNight']>
>;

// 重量級 starry-night seam は最初の着色 transform まで読み込まない。composition root の
// 静的 import に戻すと、root を参照するだけの入口へ +417.5 KB raw が再混入する。
// Promise と seam の factory はこの composition root の singleton renderer と同じ
// モジュール寿命で一つだけ保持するため、scope 付き描画が processor を都度構築しても
// 文法初期化は走り直さない。import または factory が reject した Promise は保持せず、
// 次の描画で seam の読み込みから再試行する。
let starryNightTransformerPromise: Promise<StarryNightTransformer> | undefined;
const rehypeStarryNight = () =>
  async function rehypeStarryNightLazy(...args: Parameters<StarryNightTransformer>) {
    const pending = (starryNightTransformerPromise ??= import('@/lib/rehype-starry-night').then(
      ({ createRehypeStarryNight }) => createRehypeStarryNight()(),
    ));
    try {
      const transformer = await pending;
      return transformer(...args);
    } catch (error) {
      // 遅れて reject した旧 Promise が、別描画の据えた再試行 Promise を消さない。
      if (starryNightTransformerPromise === pending) starryNightTransformerPromise = undefined;
      throw error;
    }
  };

export const renderDescription = createRenderer({
  profiles: {
    // github profile = 共有 core そのもの (GFM ＋ alerts ＋ 着色 ＋ sanitize ＋ cache ＋ SSR 契約)
    github: {
      // remarkKfmMermaid は remark 層で code(lang=mermaid) を捕まえて hName を据えるため、
      // rehype 層 (starry-night / 将来の language-* 拡張) は mermaid フェンスを見ない。
      remarkPlugins: [remarkGfm, remarkKoyoriAlerts, remarkKfmMermaid],
      // GitHub 同様のコードブロック着色。rehype 層 (remark-rehype の後段) に挿す。
      rehypePlugins: [rehypeStarryNight],
    },
    // Phase 2 seam: kfm profile はここへ remark / rehype 層を足す (コアは不変)。
  },
  sanitizeSchemas: [
    gfmSanitizeSchema,
    koyoriAlertsSanitizeSchema,
    starryNightSanitizeSchema,
    kfmMermaidSanitizeSchema,
  ],
  contentConfig,
  // config の既定 profile を描画既定へ実際に接続する (contentConfig はキャッシュキー用の
  // 不透明値でしかないため、ここで渡さない限り defaultProfile は描画に効かない)
  defaultProfile: contentConfig.defaultProfile,
});
