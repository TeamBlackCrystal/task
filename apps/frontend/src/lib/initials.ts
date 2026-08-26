/**
 * 表示名からアバターのフォールバック文字を作る。
 *
 * `String.prototype.slice` は UTF-16 コードユニット単位のため、絵文字などの
 * サロゲートペアを途中で分割して壊れた文字を返す。コードポイント単位で切り出す。
 */
export function avatarInitials(name: string, count = 2) {
  return Array.from(name).slice(0, count).join('').toUpperCase();
}
