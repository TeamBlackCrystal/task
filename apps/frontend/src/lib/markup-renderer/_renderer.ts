/**
 * _renderer.ts — controlled pipeline のコア (createRenderer)。
 *
 * pipeline: remark-parse → (profile 別 remark 層) → remark-rehype → (profile 別 rehype 層)
 *           → rehype-stringify → DOMPurify (構造専任) → HTML 文字列。
 * - allowDangerousHtml は使わない。mdast の生 html ノードは remark-rehype 既定で黙って
 *   消えるため、プラグインは data.hName / hProperties の型付き emit のみ行う契約。
 * - コアはプラグインを import しない。profile ごとの remark / rehype 層と sanitize スキーマは
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
  /**
   * remark-rehype と rehype-stringify の間に挿す rehype 層 (省略 = なし)。
   * async transformer を持つプラグイン (rehype-starry-night 等) も可 — process() が
   * await する。プラグイン factory 自体は同期である前提 (unified の use() 契約どおり)
   * のため、processor 構築 (getProcessor) は同期のまま。
   * 注意: scope 付き描画は processor を都度構築する (getProcessor 参照) ため、attach の
   * たびに高い初期化を始めるプラグインをそのまま渡すと初期化が描画回数ぶん走る。
   * 重い async 初期化は factory 側で共有・自己回収すること
   * (実例: rehype-starry-night/index.ts の createRehypeStarryNight)。
   */
  readonly rehypePlugins?: PluggableList;
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
   * fn / fnref を `-` 区切りセグメントとして含む scope も throw
   * (細工した脚注 label との id 衝突で scope 分離が破れるため)。
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

// scope 分離の不変条件を守る追加制約。脚注 id は `user-content-<scope>-fn-<label>` で、
// label は利用者入力ゆえ `-fn-` を含み得る。scope が `-` 区切りセグメントとして fn / fnref
// を含むと、別 scope (または scope なし) の細工 label と id が一致し得る
// (例: scope `a-fn-b` の label `c` と scope `a` の label `b-fn-c` は同一 id)。
// scope 側からセグメント fn / fnref を禁止すれば、id 中で最初に現れる区切り済み
// fn / fnref トークンが必ずマーカー由来となり、scope 境界が一意に復元できて衝突は起きない。
const SCOPE_RESERVED_SEGMENT_RE = /(^|-)(fn|fnref)(-|$)/;

function buildProcessor(definition: ProfileDefinition, clobberPrefix: string) {
  return unified()
    .use(remarkParse)
    .use(definition.remarkPlugins)
    .use(remarkRehype, { clobberPrefix })
    .use(definition.rehypePlugins ?? [])
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
function describePluggableList(plugins: PluggableList): string[] {
  return plugins.map((plugin) => {
    if (Array.isArray(plugin)) {
      const [fn, ...settings] = plugin;
      const name = typeof fn === 'function' ? fn.name : JSON.stringify(fn);
      return `${name}(${JSON.stringify(settings)})`;
    }
    return typeof plugin === 'function' ? plugin.name : JSON.stringify(plugin);
  });
}

function buildPipelineFingerprint(options: CreateRendererOptions): string {
  // remark 層と rehype 層を別キーで焼き込む。rehype 層を見ないと、rehypePlugins だけが
  // 違う renderer が同一キーを作り、旧規則で通った HTML を返す (kfm-cache テストで固定)。
  const pluginNames = Object.fromEntries(
    Object.entries(options.profiles).map(([profile, definition]) => [
      profile,
      {
        remark: describePluggableList(definition.remarkPlugins),
        rehype: describePluggableList(definition.rehypePlugins ?? []),
      },
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
    // 都度構築する — この「構築 = プラグイン合成のみで軽い」が成り立つのは、rehype 層の
    // 高い初期化 (starry-night の WASM＋文法登録) がプラグイン factory 側で renderer
    // スコープ共有されている前提 (rehype-starry-night/index.ts)。attach ごとに初期化を
    // 始めるプラグインを直接渡すとこの前提が崩れる (ProfileDefinition.rehypePlugins の
    // 注意書きを参照)。
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
    if (scope !== undefined && SCOPE_RESERVED_SEGMENT_RE.test(scope)) {
      throw new Error(
        `[markup-renderer] scope "${scope}" must not contain a "fn" / "fnref" segment ` +
          '(crafted footnote labels could collide across scopes)',
      );
    }
    // 改行を LF へ正規化する (\r\n と lone \r の両方。micromark はどちらも行末として
    // 解釈するが、text 値には原文の改行がそのまま残る)。正規化しないと (1) 行末 $ anchor
    // で text 値を照合するプラグイン (alerts の MARKER_RE 等) が CRLF で不成立、
    // (2) soft break の \r が HTML へ素通りし LF 版と CRLF 版が別 HTML になる。
    // キー構築より前に行うことで、同一文書の改行コード違いが同一キャッシュエントリへ
    // 畳まれる (後ろへ動かすと kfm-renderer の改行コード不変条件テストが落ちる)。
    const normalized = text.replace(/\r\n?/g, '\n');
    const clobberPrefix =
      scope === undefined ? DEFAULT_CLOBBER_PREFIX : `${DEFAULT_CLOBBER_PREFIX}${scope}-`;
    // scope '' は上の検証で throw 済みのため、キーの空文字は「scope なし」と一意に対応する
    const key = buildCacheKey(fingerprint, profile, scope ?? '', contentConfigJson, normalized);
    const cached = cache.get(key);
    if (cached !== undefined) return cached;
    const processor = getProcessor(profile, clobberPrefix);
    let rendered: string;
    try {
      rendered = String(await processor.process(normalized));
    } catch (error) {
      // process 失敗は「捨てて再試行」。renderDescription はプロセス全体で共有される
      // singleton のため、失敗した実体を永久保持するとプロセス再起動まで復旧不能になる。
      // 回収は二層:
      // (1) 共有される starry-night 実体はプラグイン factory 側が transformer の reject
      //     時に自分で捨てて作り直す。初期化 reject だけを識別する upstream の口が無く、
      //     transform 例外も対象になる点は同モジュールの契約コメントを参照。
      // (2) コア側は失敗した processor の memoize を破棄し、次回 render に再構築させる
      //     (再構築は失敗時のみ発生し、成功するまで cache.set に到達しないので誤った
      //     HTML が残ることはない)。
      // instance guard は、遅れて reject した旧 processor が別 render の据えた新しい
      // memoize を巻き添えで破棄するのを防ぐ。scope 付き描画の processor は
      // memoize されないので、この guard は自然に空振りする (削除対象がない)。
      if (processorCache.get(profile) === processor) processorCache.delete(profile);
      throw error;
    }
    const html = sanitize(rendered);
    cache.set(key, html);
    return html;
  };
}
