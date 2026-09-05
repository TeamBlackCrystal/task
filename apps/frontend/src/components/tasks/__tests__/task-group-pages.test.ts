import { describe, expect, it, vi } from 'vitest';

import {
  nextGroupCursor,
  toTaskGroup,
  type TaskGroupPage,
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

/** まだ続きがあるページ。続きの有無はサーバの next_cursor で示される */
function page(offset: number, count: number, total: number, nextCursor: string): TaskGroupPage {
  return {
    data: {
      tasks: Array.from({ length: count }, (_, i) => task(`task-${offset + i}`)),
      total,
      next_cursor: nextCursor,
    },
  };
}

/** 取り切ったページ（next_cursor が null） */
function lastPage(offset: number, count: number, total: number): TaskGroupPage {
  return {
    data: {
      tasks: Array.from({ length: count }, (_, i) => task(`task-${offset + i}`)),
      total,
      next_cursor: null,
    },
  };
}

describe('toTaskGroup', () => {
  it('next_cursor があれば もっと見る を出す', () => {
    const group = toTaskGroup(status, [page(0, PAGE_SIZE, 75, 'c1')]);
    expect(group.tasks).toHaveLength(20);
    expect(group.total).toBe(75);
    expect(group.hasMore).toBe(true);
  });

  // 取得中のページを判定に混ぜると、押した直後にボタンが消えて何も出なくなる
  it('次のページを取得中でも もっと見る を消さない', () => {
    const group = toTaskGroup(status, [page(0, PAGE_SIZE, 75, 'c1'), { isLoading: true }]);
    expect(group.isLoading).toBe(true);
    expect(group.hasMore).toBe(true);
    expect(group.tasks).toHaveLength(20);
  });

  it('サーバの上限（200 件）を超えても進める', () => {
    // 20 件ずつ 10 ページ = 200 件。limit を伸ばす方式だとここで止まっていた
    const pages = Array.from({ length: 10 }, (_, i) =>
      page(i * PAGE_SIZE, PAGE_SIZE, 275, `c${i + 1}`),
    );
    const group = toTaskGroup(status, pages);
    expect(group.tasks).toHaveLength(200);
    expect(group.hasMore).toBe(true);
  });

  it('next_cursor が null になったら取り切ったと見る', () => {
    const group = toTaskGroup(status, [page(0, PAGE_SIZE, 35, 'c1'), lastPage(20, 15, 35)]);
    expect(group.tasks).toHaveLength(35);
    expect(group.hasMore).toBe(false);
  });

  // 欠落の再発ガード。offset 方式では「取得済み < total かつ最後のページが埋まっていない」で
  // 打ち切っていたので、境界で 1 件飛んだまま もっと見る が消えていた。カーソルでは
  // 「まだあるか」をサーバが直接答えるので、total と食い違っても取りこぼさない
  it('取得済みが total に届いていなくても、next_cursor が null なら終わる', () => {
    const group = toTaskGroup(status, [lastPage(0, 19, 20)]);
    expect(group.tasks).toHaveLength(19);
    expect(group.hasMore).toBe(false);
  });

  it('ページが埋まっていても next_cursor があるかぎり続きを出す', () => {
    // 取得済み(20) が total(12) を上回るケース（読んでいるあいだに他の人が削除した）
    const group = toTaskGroup(status, [page(0, PAGE_SIZE, 12, 'c1')]);
    expect(group.hasMore).toBe(true);
  });

  it('失敗しているあいだは もっと見る を隠して再試行に寄せる', () => {
    const group = toTaskGroup(status, [page(0, PAGE_SIZE, 75, 'c1'), { isError: true }]);
    expect(group.isError).toBe(true);
    expect(group.hasMore).toBe(false);
    // 取得済みのページは残す（失敗で行が消えない）
    expect(group.tasks).toHaveLength(20);
  });

  // 途中のページだけ落ちた場合。ここで先へ進ませると、穴を飛ばしたまま
  // 「もっと見る」を押し続けることになる
  it('途中のページが落ちていたら、後ろが取れていても もっと見る を出さない', () => {
    const group = toTaskGroup(status, [
      page(0, PAGE_SIZE, 75, 'c1'),
      { isError: true },
      page(40, PAGE_SIZE, 75, 'c3'),
    ]);
    expect(group.hasMore).toBe(false);
    expect(group.isError).toBe(true);
  });

  it('retry は保持している全ページを取り直す', () => {
    const first = vi.fn();
    const second = vi.fn();
    const group = toTaskGroup(status, [
      { ...page(0, PAGE_SIZE, 75, 'c1'), refetch: first },
      { isError: true, refetch: second },
    ]);
    group.retry();
    expect(first).toHaveBeenCalledTimes(1);
    expect(second).toHaveBeenCalledTimes(1);
  });

  // カーソルでも、優先度・期限で並べるとキー自体が動くので重複はありうる
  it('ページをまたいで重複したタスクは 1 件にする', () => {
    const group = toTaskGroup(status, [
      page(0, 3, 6, 'c1'),
      { data: { tasks: [task('task-2'), task('task-9')], total: 6, next_cursor: null } },
    ]);
    expect(group.tasks.map((t) => t.id)).toEqual(['task-0', 'task-1', 'task-2', 'task-9']);
  });

  it('他のステータスのぶん（null）が混ざっても無視する', () => {
    const group = toTaskGroup(status, [null, page(0, PAGE_SIZE, 75, 'c1'), undefined]);
    expect(group.tasks).toHaveLength(20);
    expect(group.total).toBe(75);
    expect(group.hasMore).toBe(true);
  });

  it('1 ページも返っていなければ もっと見る を出さない', () => {
    const group = toTaskGroup(status, [{ isLoading: true }]);
    expect(group.tasks).toEqual([]);
    expect(group.total).toBe(0);
    expect(group.hasMore).toBe(false);
  });
});

describe('nextGroupCursor', () => {
  it('返ってきた最後のページの next_cursor を返す', () => {
    expect(nextGroupCursor([page(0, PAGE_SIZE, 75, 'c1'), page(20, PAGE_SIZE, 75, 'c2')])).toBe(
      'c2',
    );
  });

  // 取得中のページを起点にすると undefined を積んで先頭ページを二重に並べてしまう
  it('取得中・失敗中のページは起点にしない', () => {
    expect(nextGroupCursor([page(0, PAGE_SIZE, 75, 'c1'), { isLoading: true }])).toBe('c1');
    expect(nextGroupCursor([page(0, PAGE_SIZE, 75, 'c1'), { isError: true }])).toBe('c1');
  });

  it('取り切っていれば null', () => {
    expect(nextGroupCursor([lastPage(0, 5, 5)])).toBeNull();
  });

  it('1 ページも返っていなければ null', () => {
    expect(nextGroupCursor([{ isLoading: true }, null])).toBeNull();
  });
});
