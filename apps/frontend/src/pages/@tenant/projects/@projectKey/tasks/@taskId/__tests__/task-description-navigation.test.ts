import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const TASK_DETAIL_URL = 'http://example.test/acme/projects/alpha/tasks/42';

const { navigateMock, reloadMock } = vi.hoisted(() => ({
  navigateMock: vi.fn(),
  reloadMock: vi.fn(),
}));

vi.mock('vike/client/router', () => ({
  navigate: navigateMock,
  reload: reloadMock,
}));

import { refreshTaskDescription } from '../task-description-navigation';

describe('task description refresh navigation', () => {
  beforeEach(() => {
    vi.stubGlobal('location', { href: TASK_DETAIL_URL });
    navigateMock.mockReset();
    reloadMock.mockReset();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('現在 URL を再描画し、保存前のスクロール位置を保ち履歴を増やさない', async () => {
    navigateMock.mockResolvedValue(undefined);

    await refreshTaskDescription();

    expect(reloadMock).not.toHaveBeenCalled();
    expect(navigateMock).toHaveBeenCalledOnce();
    expect(navigateMock).toHaveBeenCalledWith(TASK_DETAIL_URL, {
      keepScrollPosition: true,
      overwriteLastHistoryEntry: true,
    });
  });

  it('再ナビゲート失敗を呼び出し元へ伝える', async () => {
    const error = new Error('navigation failed');
    navigateMock.mockRejectedValue(error);

    await expect(refreshTaskDescription()).rejects.toBe(error);
    expect(reloadMock).not.toHaveBeenCalled();
  });
});
