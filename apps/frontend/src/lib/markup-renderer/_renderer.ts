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
  /** テスト用 DI: L1 cache の差し替え (既定は createL1Cache) */
  readonly cache?: LRUCache<string, string>;
};

export type RenderOptions = {
  /** 既定 = github (最も忠実・装飾なし。装飾は opt-in) */
  readonly profile?: KfmProfile;
};

export type RenderDescription = (text: string, options?: RenderOptions) => Promise<string>;

function buildProcessor(definition: ProfileDefinition) {
  return unified()
    .use(remarkParse)
    .use(definition.remarkPlugins)
    .use(remarkRehype)
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

  function getProcessor(profile: KfmProfile): BuiltProcessor {
    const memoized = processorCache.get(profile);
    if (memoized) return memoized;
    const definition = options.profiles[profile];
    if (!definition) {
      throw new Error(`[markup-renderer] profile "${profile}" is not configured`);
    }
    const processor = buildProcessor(definition);
    processorCache.set(profile, processor);
    return processor;
  }

  return async function renderDescription(text, renderOptions = {}) {
    const profile = renderOptions.profile ?? 'github';
    const key = buildCacheKey(fingerprint, profile, contentConfigJson, text);
    const cached = cache.get(key);
    if (cached !== undefined) return cached;
    const html = sanitize(String(await getProcessor(profile).process(text)));
    cache.set(key, html);
    return html;
  };
}
