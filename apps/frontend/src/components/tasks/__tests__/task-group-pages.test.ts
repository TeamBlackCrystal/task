import { describe, expect, it, vi } from 'vitest';

import { toTaskGroup, type TaskGroupPage } from '@/components/tasks/task-group-pages';
import type { components } from '@/generated/api';

type StatusResponse = components['schemas']['ProjectStatusResponse'];
type TaskResponse = components['schemas']['TaskResponse'];

const status = {
  id: 'status-todo',
  name: 'Todo',
  color: '#94a3b8',
  position: 0,
  is_default: true,
  is_done_state: false,
  project_id: 'project-1',
  created_at: '2026-06-01T00:00:00Z',
} satisfies StatusResponse;

const PAGE_SIZE = 20;

function task(id: string) {
  return { id } as TaskResponse;
}

/** `limit` 件ちょうどのページ。境界そのものを使うのは「埋まったページ」の表現に必要なため */
function fullPage(offset: number, total: number): TaskGroupPage {
  return {
    data: {
      tasks: Array.from({ length: PAGE_SIZE }, (_, i) => task(`task-${offset + i}`)),
      total,
    },
  };
}

function partialPage(offset: number, count: number, total: number): TaskGroupPage {
  return {
    data: { tasks: Array.from({ length: count }, (_, i) => task(`task-${offset + i}`)), total },
  };
}

describe('toTaskGroup', () => {
  it('埋まったページの後ろに残りがあれば もっと見る を出す', () => {
    const group = toTaskGroup(status, [fullPage(0, 75)], PAGE_SIZE);
    expect(group.tasks).toHaveLength(20);
    expect(group.total).toBe(75);
    expect(group.hasMore).toBe(true);
  });

  // 取得中のページを判定に混ぜると、押した直後にボタンが消えて何も出なくなる
  it('次のページを取得中でも もっと見る を消さない', () => {
    const group = toTaskGroup(status, [fullPage(0, 75), { isLoading: true }], PAGE_SIZE);
    expect(group.isLoading).toBe(true);
    expect(group.hasMore).toBe(true);
    expect(group.tasks).toHaveLength(20);
  });

  it('サーバの上限（200 件）を超えても進める', () => {
    // 20 件ずつ 10 ページ = 200 件。limit を伸ばす方式だとここで止まっていた
    const pages = Array.from({ length: 10 }, (_, page) => fullPage(page * PAGE_SIZE, 275));
    const group = toTaskGroup(status, pages, PAGE_SIZE);
    expect(group.tasks).toHaveLength(200);
    expect(group.hasMore).toBe(true);
  });

  it('最後のページが埋まっていなければ取り切ったと見る', () => {
    const group = toTaskGroup(status, [fullPage(0, 35), partialPage(20, 15, 35)], PAGE_SIZE);
    expect(group.tasks).toHaveLength(35);
    expect(group.hasMore).toBe(false);
  });

  it('件数が減っていても もっと見る を残さない', () => {
    // total が取得済みより小さくなるケース（他の人が削除した後）
    const group = toTaskGroup(status, [fullPage(0, 12)], PAGE_SIZE);
    expect(group.hasMore).toBe(false);
  });

  it('失敗しているあいだは もっと見る を隠して再試行に寄せる', () => {
    const group = toTaskGroup(status, [fullPage(0, 75), { isError: true }], PAGE_SIZE);
    expect(group.isError).toBe(true);
    expect(group.hasMore).toBe(false);
    // 取得済みのページは残す（失敗で行が消えない）
    expect(group.tasks).toHaveLength(20);
  });

  // 途中のページだけ落ちた場合。ここで先へ進ませると、穴を飛ばしたまま
  // 「もっと見る」を押し続けることになる
  it('途中のページが落ちていたら、後ろが取れていても もっと見る を出さない', () => {
    const group = toTaskGroup(
      status,
      [fullPage(0, 75), { isError: true }, fullPage(40, 75)],
      PAGE_SIZE,
    );
    expect(group.hasMore).toBe(false);
    expect(group.isError).toBe(true);
  });

  it('retry は保持している全ページを取り直す', () => {
    const first = vi.fn();
    const second = vi.fn();
    const group = toTaskGroup(
      status,
      [
        { ...fullPage(0, 75), refetch: first },
        { isError: true, refetch: second },
      ],
      PAGE_SIZE,
    );
    group.retry();
    expect(first).toHaveBeenCalledTimes(1);
    expect(second).toHaveBeenCalledTimes(1);
  });

  it('ページをまたいで重複したタスクは 1 件にする', () => {
    const group = toTaskGroup(
      status,
      [partialPage(0, 3, 6), { data: { tasks: [task('task-2'), task('task-9')], total: 6 } }],
      PAGE_SIZE,
    );
    expect(group.tasks.map((t) => t.id)).toEqual(['task-0', 'task-1', 'task-2', 'task-9']);
  });

  it('他のステータスのぶん（null）が混ざっても無視する', () => {
    const group = toTaskGroup(status, [null, fullPage(0, 75), undefined], PAGE_SIZE);
    expect(group.tasks).toHaveLength(20);
    expect(group.total).toBe(75);
    expect(group.hasMore).toBe(true);
  });

  it('1 ページも返っていなければ もっと見る を出さない', () => {
    const group = toTaskGroup(status, [{ isLoading: true }], PAGE_SIZE);
    expect(group.tasks).toEqual([]);
    expect(group.total).toBe(0);
    expect(group.hasMore).toBe(false);
  });
});
