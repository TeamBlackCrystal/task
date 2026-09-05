import { describe, expect, it, vi } from 'vitest';

import {
  toTaskGroup,
  type TaskGroupPage,
  type TaskGroupQueryState,
} from '@/components/tasks/task-group-pages';
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

function page(offset: number, count: number, total: number, nextCursor: string | null) {
  return {
    tasks: Array.from({ length: count }, (_, i) => task(`task-${offset + i}`)),
    total,
    next_cursor: nextCursor,
  } satisfies TaskGroupPage;
}

/**
 * infinite query の状態。`hasNextPage` は TanStack が `getNextPageParam`
 * （= 最後のページの next_cursor）から畳んだ結果なので、ここでも合わせて渡す。
 */
function state(pages: TaskGroupPage[], extra: Partial<TaskGroupQueryState> = {}) {
  return {
    data: { pages },
    hasNextPage: pages.at(-1)?.next_cursor != null,
    ...extra,
  } satisfies TaskGroupQueryState;
}

describe('toTaskGroup', () => {
  it('続きがあれば もっと見る を出す', () => {
    const group = toTaskGroup(status, state([page(0, PAGE_SIZE, 75, 'c1')]));
    expect(group.tasks).toHaveLength(20);
    expect(group.total).toBe(75);
    expect(group.hasMore).toBe(true);
  });

  // 押した直後に何も起きていないように見えると、続けて押されて同じページが二重に積まれる
  it('次のページを取得中でも もっと見る を消さず、読み込み中にする', () => {
    const group = toTaskGroup(
      status,
      state([page(0, PAGE_SIZE, 75, 'c1')], { isFetchingNextPage: true }),
    );
    expect(group.isLoading).toBe(true);
    expect(group.hasMore).toBe(true);
    expect(group.tasks).toHaveLength(20);
  });

  it('サーバの上限（200 件）を超えても進める', () => {
    // 20 件ずつ 10 ページ = 200 件。limit を伸ばす方式だとここで止まっていた
    const pages = Array.from({ length: 10 }, (_, i) =>
      page(i * PAGE_SIZE, PAGE_SIZE, 275, `c${i + 1}`),
    );
    const group = toTaskGroup(status, state(pages));
    expect(group.tasks).toHaveLength(200);
    expect(group.hasMore).toBe(true);
  });

  it('next_cursor が null になったら取り切ったと見る', () => {
    const group = toTaskGroup(
      status,
      state([page(0, PAGE_SIZE, 35, 'c1'), page(20, 15, 35, null)]),
    );
    expect(group.tasks).toHaveLength(35);
    expect(group.hasMore).toBe(false);
  });

  // 欠落の再発ガード。offset 方式では「取得済み < total かつ最後のページが埋まっていない」で
  // 打ち切っていたので、境界で 1 件飛んだまま もっと見る が消えていた
  it('取得済みが total に届いていなくても、続きが無ければ終わる', () => {
    const group = toTaskGroup(status, state([page(0, 19, 20, null)]));
    expect(group.tasks).toHaveLength(19);
    expect(group.hasMore).toBe(false);
  });

  it('ページが埋まっていても続きがあるかぎり もっと見る を出す', () => {
    // 取得済み(20) が total(12) を上回るケース（読んでいるあいだに他の人が削除した）
    const group = toTaskGroup(status, state([page(0, PAGE_SIZE, 12, 'c1')]));
    expect(group.hasMore).toBe(true);
  });

  // 穴を飛ばして先へ進ませない。失敗しているあいだは再試行へ寄せる
  it('失敗しているあいだは もっと見る を隠す', () => {
    const group = toTaskGroup(status, state([page(0, PAGE_SIZE, 75, 'c1')], { isError: true }));
    expect(group.isError).toBe(true);
    expect(group.hasMore).toBe(false);
    // 取得済みのページは残す（失敗で行が消えない）
    expect(group.tasks).toHaveLength(20);
  });

  it('retry は infinite query を取り直す（全ページを先頭から継ぎ直す）', () => {
    const refetch = vi.fn();
    const group = toTaskGroup(status, state([page(0, PAGE_SIZE, 75, 'c1')], { refetch }));
    group.retry();
    expect(refetch).toHaveBeenCalledTimes(1);
  });

  it('loadMore は次のページを足す', () => {
    const fetchNextPage = vi.fn();
    const group = toTaskGroup(status, state([page(0, PAGE_SIZE, 75, 'c1')], { fetchNextPage }));
    group.loadMore();
    expect(fetchNextPage).toHaveBeenCalledTimes(1);
  });

  // カーソルでも、優先度・期限で並べるとキー自体が動くので重複はありうる
  it('ページをまたいで重複したタスクは 1 件にする', () => {
    const group = toTaskGroup(
      status,
      state([
        page(0, 3, 6, 'c1'),
        { tasks: [task('task-2'), task('task-9')], total: 6, next_cursor: null },
      ]),
    );
    expect(group.tasks.map((t) => t.id)).toEqual(['task-0', 'task-1', 'task-2', 'task-9']);
  });

  it('1 ページも返っていなければ もっと見る を出さない', () => {
    const group = toTaskGroup(status, { isLoading: true });
    expect(group.tasks).toEqual([]);
    expect(group.total).toBe(0);
    expect(group.isLoading).toBe(true);
    expect(group.hasMore).toBe(false);
  });
});
