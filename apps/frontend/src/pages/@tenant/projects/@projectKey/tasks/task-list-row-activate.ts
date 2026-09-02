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

/**
 * 行の操作を「別のタブ・ウィンドウで開く」として扱うか。
 *
 * 疑似要素で行全体を覆っていた間は、行のどこでも実リンクへの操作だったので
 * Ctrl / Cmd + クリックと中クリックで詳細が別タブに開けた。判定を click へ移すと
 * タイトルの文字以外でその操作が何も起きなくなるため、行側で補う。
 *
 * Alt + クリックは入れない。ブラウザによってはダウンロードの合図で、別タブを開く
 * 操作ではない。
 */
export function shouldOpenRowInNewTab(event: MouseEvent): boolean {
  const target = event.target as Element | null;
  // 行内の操作要素は、その要素本来の動作（リンクなら別タブ）に任せる
  if (target?.closest?.(ROW_INTERACTIVE_SELECTOR)) return false;
  // 中クリック
  if (event.button === 1) return true;
  // 左クリック + 修飾キー
  return event.button === 0 && (event.metaKey || event.ctrlKey || event.shiftKey);
}
