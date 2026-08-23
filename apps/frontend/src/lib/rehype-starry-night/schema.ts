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
