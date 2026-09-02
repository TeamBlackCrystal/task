/**
 * モバイルでサイドバーからページへ移ったときに、サイドバーを閉じるかどうか。
 *
 * モバイルのサイドバーはページに重なって出るので、開いたままだと遷移先が隠れ、
 * 移動したことが一見して分からない。個々のリンクに閉じる処理を付けると追加の
 * たびに漏れるため、ナビが集まる要素でまとめて拾う。
 *
 * 委譲の下に入らないリンク（`DropdownMenuPortal` でサイドバーの外へ出る
 * ユーザーメニューなど）は、同じ判定をそのリンク側で呼ぶ。
 */
export function shouldCloseSidebarOnNavigate(event: MouseEvent, isMobile: boolean): boolean {
  // デスクトップのサイドバーは常設で、ページと重ならないので閉じない
  if (!isMobile) return false;
  // 修飾キー付き・左クリック以外は別タブで開くだけで、このページは動かない
  if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) {
    return false;
  }
  return !!(event.target as Element | null)?.closest?.('a');
}

/**
 * リンクを踏まない遷移（ボタンから `navigate` を呼ぶ導線）のあと始末。
 *
 * 上のイベント委譲は `closest('a')` で拾うので、プログラム遷移するボタンは掛からない。
 * サイドバーの「プロジェクトを作成」はこれに当たり、遷移してもサイドバーが開いたまま
 * 作成画面を覆っていた。遷移する側から呼んで閉じる。
 */
export function closeSidebarForProgrammaticNavigate(
  isMobile: boolean,
  setOpenMobile: (value: boolean) => void,
): void {
  if (isMobile) setOpenMobile(false);
}
