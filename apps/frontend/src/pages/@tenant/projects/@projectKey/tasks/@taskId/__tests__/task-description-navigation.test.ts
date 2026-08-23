import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

/** タスク詳細 URL の pathname 形（実装の window.location.href 式に依存しない固定値）。 */
const TASK_DETAIL_PATH = '/acme/projects/alpha/tasks/42';
const TASK_DETAIL_URL = `http://example.test${TASK_DETAIL_PATH}`;

const { navigateMock } = vi.hoisted(() => ({
  navigateMock: vi.fn(),
}));

vi.mock('vike/client/router', () => ({
  navigate: navigateMock,
}));

import {
  TASK_DESCRIPTION_REFRESH_NAVIGATE_OPTIONS,
  refreshTaskDescription,
} from '../task-description-navigation';

describe('task description refresh navigation', () => {
  beforeEach(() => {
    vi.stubGlobal('location', { href: TASK_DETAIL_URL });
    navigateMock.mockReset();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('現在 URL を再描画し、保存前のスクロール位置を保ち履歴を増やさない', async () => {
    navigateMock.mockResolvedValue(undefined);

    await refreshTaskDescription();

    expect(navigateMock).toHaveBeenCalledOnce();
    const [calledHref, options] = navigateMock.mock.calls[0]!;
    const calledUrl = new URL(calledHref);
    expect(calledUrl.pathname).toBe(TASK_DETAIL_PATH);
    expect(calledUrl.hostname).toBe('example.test');
    expect(options).toBe(TASK_DESCRIPTION_REFRESH_NAVIGATE_OPTIONS);
  });

  it('再ナビゲート失敗を呼び出し元へ伝える', async () => {
    const error = new Error('navigation failed');
    navigateMock.mockRejectedValue(error);

    await expect(refreshTaskDescription()).rejects.toBe(error);
  });
});
