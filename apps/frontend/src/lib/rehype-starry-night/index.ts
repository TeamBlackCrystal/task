/**
 * KFM のコードブロック着色層 — rehype-starry-night の薄いラッパ。
 * 消費側は rehype-starry-night / @wooorm/starry-night を直接 import せず本モジュールを
 * 経由する (remark-gfm と同じ seam)。制御層は未実装ゆえ今は常時読み込みだが、将来
 * トグルを足すときは composition root (markup-renderer/index.ts) の注入だけを変えればよく、
 * コア (_renderer / _sanitize / _cache) は本モジュールを import しない構造を保つ。
 *
 * rehype-starry-night@2.2.0 実物確認に基づく契約:
 * - 文法は既定の common (GitHub 頻出言語)。all は bundle が桁で膨れるため使わない。
 * - upstream factory (attacher) は呼ばれた時点で createStarryNight (async: onig.wasm ＋
 *   common 文法一式の登録) を開始し、その Promise を返す transformer の closure に保持する。
 *   よって factory 呼び出し回数 = 文法初期化回数であり、これが着色の費用の本体
 *   (実測: 初回 46.7ms・以降 15〜17ms/回)。processor 構築 (unified の use/freeze) 自体は
 *   同期・軽量のまま。共有と失敗回収は下の createRehypeStarryNight を参照。
 * - onig.wasm は node 条件では vscode-oniguruma 同梱物の fs 読み、browser 条件では
 *   https://esm.sh/vscode-oniguruma@2 への fetch (#get-oniguruma の条件分岐)。SSR 専用の
 *   現状で外部 fetch は発生しないが、client で初期化する変更はこの外部依存を踏む。
 * - 変換は `<code class="language-*">` の子を `pl-*` クラスの span 群へ置換するのみ。
 *   inline style・新規タグ・wrapper 要素は一切出さない (FORBID_ATTR: ['style'] 契約と一枚岩)。
 * - 未知言語フェンスは変換されず素のコードブロックのまま (中身はエスケープ済みテキスト
 *   維持・vfile message が付くだけでエラーにはならない)。
 * - 巨大フェンスに行数上限は設けていない (実測 2000 行 ≈ 112ms/回。再描画は L1 キャッシュが
 *   吸収する。上限を切る場合はここではなく入力側の文書サイズ制限で行う)。
 * - 見た目はサイドカー style.css を消費側が明示 import する (alerts と同じ方式)。
 */
import upstreamRehypeStarryNight from 'rehype-starry-night';

type UpstreamTransformer = ReturnType<typeof upstreamRehypeStarryNight>;

/**
 * starry-night 実体を renderer スコープで一つに共有するプラグイン factory。
 *
 * composition root が renderer 1 つにつき本 factory を 1 回呼び、戻り値のプラグインを
 * rehypePlugins へ渡す。scope 付き描画は clobberPrefix が異なるため processor を都度
 * 構築する (_renderer.ts getProcessor) が、高いのは processor ではなく createStarryNight
 * (WASM ＋ 文法登録) — 共有しないとコメント N 件のページ 1 リクエストで N 回初期化が走る。
 * 着色は profile / clobberPrefix と無関係なので、transformer が抱える starry-night
 * Promise を全 processor で共有しても出力は変わらない。upstream closure の可変状態
 * `checked` も共有されるため missingScopes 警告は renderer につき初回の 1 度だけになるが、
 * renderDescription は vfile message を返さないので描画結果への影響はない
 * (rehype-starry-night@2.2.0 lib/index.js 実物確認)。
 *
 * 共有はモジュールレベルではなく renderer スコープ (factory closure) に置く。プロセス
 * 共有にすると renderer を作り直しても実体が残り、テスト間の隔離と寿命の所有権
 * (renderer と共に捨てられること) が崩れるため。
 *
 * 失敗回収: createStarryNight が一度失敗すると upstream transformer は poisoned promise
 * を抱えて以後の全描画で reject し続ける。共有はこれを単一障害点に昇格させるため、
 * transformer の reject 時は共有実体を捨てて次の描画で作り直す。これは初期化 reject
 * だけを識別する口が upstream に無いため、入力依存の transform 例外も破棄対象になる
 * (未知言語は例外でなく vfile message)。実体の解決を attach 時ではなく transform 時に
 * 行うことで、memoize 済み processor も次回に新実体を掴む。instance guard は、遅れて
 * reject した旧実体が別描画の据えた新実体を巻き添えで破棄するのを防ぐ
 * (_renderer.ts の processorCache guard と同型)。
 *
 * upstream options は意図して受け取らない。_renderer.ts の fingerprint は plugin の
 * 関数名しか見ないため、closure に閉じた options は設定差があってもキャッシュキーに
 * 現れず、CreateRendererOptions.cache を共有する renderer 間で他構成の HTML を返し得る。
 * options の口を開けるときは fingerprint への直列化とキー分離試験 (kfm-cache.test.ts)
 * を同じ変更で入れること。
 */
export function createRehypeStarryNight(): () => UpstreamTransformer {
  let shared: UpstreamTransformer | undefined;
  return function rehypeStarryNightShared(): UpstreamTransformer {
    return async function transform(...args: Parameters<UpstreamTransformer>) {
      const instance = (shared ??= upstreamRehypeStarryNight());
      try {
        return await instance(...args);
      } catch (error) {
        if (shared === instance) shared = undefined;
        throw error;
      }
    };
  };
}

/**
 * starry-night が emit する class の許可パターン (完全一致 allowlist の classPatterns 側)。
 * 実測と機械列挙で決定 (推測ではない):
 * - 20 言語サンプルの実出力から 14 class を観測 (pl-c / pl-c1 / pl-k / pl-s / pl-smi 等)
 * - @wooorm/starry-night/lib/theme.js の scope→class 対応表の全値域 34 class が
 *   すべて /^pl-[a-z0-9]+$/ に一致することを機械検証 (観測 14 class ⊆ 全値域 34 class)
 * - style/light.css のセレクタ列挙 33 class も同パターン内 (both.css と同一集合を機械照合済)
 * 小文字英数のみ・アンカー付きのため、アプリ側 class の騙りには転用できない。
 * コードフェンス自体の `language-*` は gfmSanitizeSchema 側の既存パターンが受け持つ。
 */
export const starryNightSanitizeSchema = {
  classPatterns: [/^pl-[a-z0-9]+$/],
} as const;
