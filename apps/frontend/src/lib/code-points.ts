/**
 * backend（validator クレートの `chars().count()`）と同じく、UTF-16 コードユニットではなく
 * Unicode コードポイント単位で文字数を数える。ずれると絵文字入りの入力で画面と API の
 * 判定が食い違う（#581 で ProfileForm に導入したものの共有版）。
 */
export function codePointLength(value: string): number {
  return Array.from(value).length;
}
