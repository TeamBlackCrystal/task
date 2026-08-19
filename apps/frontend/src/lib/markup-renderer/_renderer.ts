/**
 * _renderer.ts — controlled pipeline のコア (createRenderer)。
 *
 * pipeline: remark-parse → (profile 別 remark 層) → remark-rehype → rehype-stringify
 *           → DOMPurify (構造専任) → HTML 文字列。
 * - allowDangerousHtml は使わない。mdast の生 html ノードは remark-rehype 既定で黙って
 *   消えるため、プラグインは data.hName / hProperties の型付き emit のみ行う契約。
 * - コアはプラグインを import しない。profile ごとの remark 層と sanitize スキーマは
 *   composition root (index.ts) が注入する。
 * - processor は profile ごとに 1 回だけ build して memoize する (N 重初期化回避)。
 */
import type { LRUCache } from 'lru-cache';
import rehypeStringify from 'rehype-stringify';
import remarkParse from 'remark-parse';
import remarkRehype from 'remark-rehype';
import type { PluggableList } from 'unified';
import { unified } from 'unified';
import { buildCacheKey, createL1Cache } from './_cache';
import type { SanitizeSchema } from './_sanitize';
import { createSanitizer } from './_sanitize';

export type KfmProfile = 'github';
// Phase 2 seam: 'kfm' (GFM ＋ alerts ＋ MFM ＋ Koyori 拡張)、将来 'gitlab' (GLFM) を
// この union に足し、createRenderer へ渡す profiles にプラグイン列を追加するだけで
// 拡張する (コア本体・sanitize・cache・SSR 契約は全 profile 共有で不変)。

export type ProfileDefinition = {
  /** 共有 core (remark-parse → remark-rehype → rehype-stringify) に挿す remark 層 */
  readonly remarkPlugins: PluggableList;
};

export type CreateRendererOptions = {
  readonly profiles: Readonly<Partial<Record<KfmProfile, ProfileDefinition>>>;
  /** 各プラグインが export する sanitize スキーマ (emit と sanitize の許可集合の単一ソース) */
  readonly sanitizeSchemas: readonly SanitizeSchema[];
  /** 解決済み content-scope config。キャッシュキーに全文が焼き込まれる */
  readonly contentConfig?: unknown;
  /**
   * profile 未指定時の既定。composition root が contentConfig.defaultProfile を渡す
   * (ここへ渡さないと config の既定が描画に反映されない)。省略時 github。
   */
  readonly defaultProfile?: KfmProfile;
  /** テスト用 DI: L1 cache の差し替え (既定は createL1Cache) */
  readonly cache?: LRUCache<string, string>;
};

export type RenderOptions = {
  /** 既定 = createRenderer の defaultProfile (それも無ければ github) */
  readonly profile?: KfmProfile;
  /**
   * 脚注 id の衝突回避 scope。同一ページへ複数の KFM 断片 (タスク本文＋コメント等) を
   * 並べる場合、断片ごとに決定的な scope (例: `comment-42`) を渡す。remark-rehype の
   * clobberPrefix へ `user-content-<scope>-` として反映され、キャッシュキーにも載る。
   * random ではなく呼び出し側の決定的識別子である理由: 同一入力→同一 HTML を保たないと
   * L1 キャッシュ前提 (SSR/CSR 同一性) が崩れるため。[A-Za-z0-9_-]+ 以外は throw。
   */
  readonly scope?: string;
};

export type RenderDescription = (text: string, options?: RenderOptions) => Promise<string>;

// remark-rehype 既定の clobberPrefix (GitHub 互換)。scope 付き描画は
// `user-content-<scope>-` へ差し替えて脚注 id (fn-* / fnref-*) の衝突を避ける。
const DEFAULT_CLOBBER_PREFIX = 'user-content-';

// scope は id 属性と URL fragment (#...) にそのまま入るため、安全な字種に限定して
// fail-closed で弾く (HTML 構造や href を scope 経由で汚染させない)。
const SCOPE_RE = /^[A-Za-z0-9_-]+$/;

function buildProcessor(definition: ProfileDefinition, clobberPrefix: string) {
  return unified()
    .use(remarkParse)
    .use(definition.remarkPlugins)
    .use(remarkRehype, { clobberPrefix })
    .use(rehypeStringify)
    .freeze();
}

type BuiltProcessor = ReturnType<typeof buildProcessor>;

/**
 * pipeline 設定の fingerprint。手動バンプではなく plugin 列・sanitize スキーマから導出し、
 * どちらかを変えるとキャッシュキーが自動的に変わって旧規則で通った HTML が失効する。
 * プロセス内 (L1) 専用 —— 関数名は minify で変わり得るため、永続 L2 を導入する際は
 * ビルドを跨いで安定な名前へ置き換えること。
 */
function buildPipelineFingerprint(options: CreateRendererOptions): string {
  const pluginNames = Object.fromEntries(
    Object.entries(options.profiles).map(([profile, definition]) => [
      profile,
      definition.remarkPlugins.map((plugin) => {
        if (Array.isArray(plugin)) {
          const [fn, ...settings] = plugin;
          const name = typeof fn === 'function' ? fn.name : JSON.stringify(fn);
          return `${name}(${JSON.stringify(settings)})`;
        }
        return typeof plugin === 'function' ? plugin.name : JSON.stringify(plugin);
      }),
    ]),
  );
  const sanitizeShape = options.sanitizeSchemas.map((schema) => ({
    tags: [...(schema.tags ?? [])],
    attrs: schema.attrs ?? {},
    classTokens: [...(schema.classTokens ?? [])],
    classPatterns: (schema.classPatterns ?? []).map(String),
  }));
  return JSON.stringify({
    core: ['remark-parse', 'remark-rehype', 'rehype-stringify'],
    plugins: pluginNames,
    sanitize: sanitizeShape,
  });
}

export function createRenderer(options: CreateRendererOptions): RenderDescription {
  const cache = options.cache ?? createL1Cache();
  const sanitize = createSanitizer(options.sanitizeSchemas);
  const fingerprint = buildPipelineFingerprint(options);
  const contentConfigJson = JSON.stringify(options.contentConfig ?? null);
  const processorCache = new Map<KfmProfile, BuiltProcessor>();

  function getDefinition(profile: KfmProfile): ProfileDefinition {
    const definition = options.profiles[profile];
    if (!definition) {
      throw new Error(`[markup-renderer] profile "${profile}" is not configured`);
    }
    return definition;
  }

  function getProcessor(profile: KfmProfile, clobberPrefix: string): BuiltProcessor {
    // memoize は既定 prefix のみ。scope の値空間は非有界 (comment id 等) で、singleton
    // の SSR プロセスに scope ごとの processor を溜めるとメモリが漏れる。scope 付きは
    // 都度構築する — 構築はプラグイン合成のみで、cache miss 時に必ず走る
    // parse＋sanitize に比べ無視できる。
    if (clobberPrefix !== DEFAULT_CLOBBER_PREFIX) {
      return buildProcessor(getDefinition(profile), clobberPrefix);
    }
    const memoized = processorCache.get(profile);
    if (memoized) return memoized;
    const processor = buildProcessor(getDefinition(profile), DEFAULT_CLOBBER_PREFIX);
    processorCache.set(profile, processor);
    return processor;
  }

  return async function renderDescription(text, renderOptions = {}) {
    const profile = renderOptions.profile ?? options.defaultProfile ?? 'github';
    const scope = renderOptions.scope;
    if (scope !== undefined && !SCOPE_RE.test(scope)) {
      throw new Error(
        `[markup-renderer] scope "${scope}" must match [A-Za-z0-9_-]+ (id / URL fragment safety)`,
      );
    }
    const clobberPrefix =
      scope === undefined ? DEFAULT_CLOBBER_PREFIX : `${DEFAULT_CLOBBER_PREFIX}${scope}-`;
    // scope '' は上の検証で throw 済みのため、キーの空文字は「scope なし」と一意に対応する
    const key = buildCacheKey(fingerprint, profile, scope ?? '', contentConfigJson, text);
    const cached = cache.get(key);
    if (cached !== undefined) return cached;
    const html = sanitize(String(await getProcessor(profile, clobberPrefix).process(text)));
    cache.set(key, html);
    return html;
  };
}
