/**
 * タスク一覧の「行のどこを押しても詳細へ入る」当たり判定。
 *
 * 以前は行のタイトルリンクに `after:absolute after:inset-0` を敷いて判定を行全体へ
 * 広げていたが、`<tr>` は WebKit で絶対配置の包含ブロックにならないため、判定が行を
 * 飛び越えてテーブル全体（`Table.vue` の `relative` なコンテナ）へ広がっていた。
 * 全行ぶんが同じ領域で重なり、DOM 順で最後の行だけが反応するので、どこをタップしても
 * 一番下のタスクが開いていた。判定を CSS から行の click へ移して、包含ブロックの
 * 扱いに依存しないようにする。
 */

/** 行内の操作要素。ここを押したときは行の遷移を起こさず、その要素の動作に任せる。 */
export const ROW_INTERACTIVE_SELECTOR = 'a,button,input,label,select,textarea,[role="checkbox"]';

/**
 * 行の click を詳細への遷移として扱うか。
 *
 * 修飾キー付き・左クリック以外は、ブラウザ本来の動作（新しいタブで開く等）に任せる。
 * 行内の操作要素は、タイトルのリンクも含めてここで抜ける。リンクは自前の click
 * ハンドラで遷移と修飾キーを扱うため、行側で二重に処理しない。
 */
export function shouldActivateRow(event: MouseEvent): boolean {
  if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) {
    return false;
  }
  const target = event.target as Element | null;
  if (target?.closest?.(ROW_INTERACTIVE_SELECTOR)) return false;
  return true;
}
