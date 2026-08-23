import { navigate } from 'vike/client/router';

/** 説明保存後の同一 URL 再ナビゲート契約（KFM 規範とテストの単一ソース）。 */
export const TASK_DESCRIPTION_REFRESH_NAVIGATE_OPTIONS = {
  keepScrollPosition: true,
  overwriteLastHistoryEntry: true,
} as const;

/** SSR 描画済みの説明を取り直しつつ、長い本文上の編集位置を維持する。 */
export function refreshTaskDescription(): Promise<void> {
  return navigate(window.location.href, TASK_DESCRIPTION_REFRESH_NAVIGATE_OPTIONS);
}
