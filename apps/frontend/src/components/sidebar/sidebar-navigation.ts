/**
 * モバイルでサイドバーからページへ移ったときに、サイドバーを閉じるかどうか。
 *
 * モバイルのサイドバーはページに重なって出るので、開いたままだと遷移先が隠れ、
 * 移動したことが一見して分からない。個々のリンクに閉じる処理を付けると追加の
 * たびに漏れるため、ナビが集まる要素でまとめて拾う。
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
