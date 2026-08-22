import { navigate } from 'vike/client/router';

/** SSR 描画済みの説明を取り直しつつ、長い本文上の編集位置を維持する。 */
export function refreshTaskDescription(): Promise<void> {
  return navigate(window.location.href, {
    keepScrollPosition: true,
    overwriteLastHistoryEntry: true,
  });
}
