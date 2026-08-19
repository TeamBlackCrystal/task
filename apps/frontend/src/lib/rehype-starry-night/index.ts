/**
 * KFM のコードブロック着色層 — rehype-starry-night の薄いラッパ。
 * 消費側は rehype-starry-night / @wooorm/starry-night を直接 import せず本モジュールを
 * 経由する (remark-gfm と同じ seam)。制御層は未実装ゆえ今は常時読み込みだが、将来
 * トグルを足すときは composition root (markup-renderer/index.ts) の注入だけを変えればよく、
 * コア (_renderer / _sanitize / _cache) は本モジュールを import しない構造を保つ。
 *
 * rehype-starry-night@2.2.0 実物確認に基づく契約:
 * - 文法は既定の common (GitHub 頻出言語)。all は bundle が桁で膨れるため使わない。
 * - プラグイン factory は同期で、createStarryNight (async) の Promise を内部保持し
 *   transformer が await する。よって processor 構築は同期のままでよい (_renderer.ts)。
 *   初期化失敗時の扱いは _renderer.ts の renderDescription を参照 (捨てて再試行)。
 * - 変換は `<code class="language-*">` の子を `pl-*` クラスの span 群へ置換するのみ。
 *   inline style・新規タグ・wrapper 要素は一切出さない (FORBID_ATTR: ['style'] 契約と一枚岩)。
 * - 未知言語フェンスは変換されず素のコードブロックのまま (中身はエスケープ済みテキスト
 *   維持・vfile message が付くだけでエラーにはならない)。
 * - 見た目はサイドカー style.css を消費側が明示 import する (alerts と同じ方式)。
 */
export { default as rehypeStarryNight } from 'rehype-starry-night';

/**
 * starry-night が emit する class の許可パターン (完全一致 allowlist の classPatterns 側)。
 * 実測と機械列挙で決定 (推測ではない):
 * - 20 言語サンプルの実出力から 14 class を観測 (pl-c / pl-c1 / pl-k / pl-s / pl-smi 等)
 * - @wooorm/starry-night/lib/theme.js の scope→class 対応表の全値域 34 class が
 *   すべて /^pl-[a-z0-9]+$/ に一致することを機械検証 (観測 14 class ⊆ 全値域 34 class)
 * - style/both.css のセレクタ列挙 33 class も同パターン内
 * 小文字英数のみ・アンカー付きのため、アプリ側 class の騙りには転用できない。
 * コードフェンス自体の `language-*` は gfmSanitizeSchema 側の既存パターンが受け持つ。
 */
export const starryNightSanitizeSchema = {
  classPatterns: [/^pl-[a-z0-9]+$/],
} as const;
