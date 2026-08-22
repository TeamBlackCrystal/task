import { beforeEach, describe, expect, it, vi } from 'vitest';

const { navigateMock } = vi.hoisted(() => ({ navigateMock: vi.fn() }));

vi.mock('vike/client/router', () => ({
  navigate: navigateMock,
}));

import { refreshTaskDescription } from '../task-description-navigation';

describe('task description refresh navigation', () => {
  beforeEach(() => {
    navigateMock.mockReset();
  });

  it('現在 URL を再描画し、保存前のスクロール位置を保つ', async () => {
    navigateMock.mockResolvedValue(undefined);

    await refreshTaskDescription();

    expect(navigateMock).toHaveBeenCalledOnce();
    expect(navigateMock).toHaveBeenCalledWith(window.location.href, {
      keepScrollPosition: true,
    });
  });

  it('再ナビゲート失敗を呼び出し元へ伝える', async () => {
    const error = new Error('navigation failed');
    navigateMock.mockRejectedValue(error);

    await expect(refreshTaskDescription()).rejects.toBe(error);
  });
});
