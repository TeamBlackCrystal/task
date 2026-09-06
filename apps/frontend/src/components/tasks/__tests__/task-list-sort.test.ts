import { describe, expect, it } from 'vitest';

import { activeTaskListSort, taskListApiSort } from '@/components/tasks/task-list-sort';

describe('taskListApiSort', () => {
  it.each([
    ['title', false, 'title_asc'],
    ['title', true, 'title_desc'],
    ['assignee', false, 'assignee_asc'],
    ['assignee', true, 'assignee_desc'],
    ['due_date', false, 'deadline_asc'],
    ['due_date', true, 'deadline_desc'],
    ['priority', false, 'priority_asc'],
    ['priority', true, 'priority_desc'],
  ])('%s の desc=%s を API の %s に変換する', (id, desc, expected) => {
    expect(taskListApiSort([{ id, desc }])).toBe(expected);
  });

  it('解除時と未対応列では既定順へ戻す', () => {
    expect(taskListApiSort([])).toBe('created_at_desc');
    expect(taskListApiSort([{ id: 'status', desc: false }])).toBe('created_at_desc');
    expect(activeTaskListSort([{ id: 'status', desc: false }])).toBeNull();
  });

  it('複数列が渡されても List で操作する先頭の列だけを使う', () => {
    expect(
      taskListApiSort([
        { id: 'priority', desc: false },
        { id: 'title', desc: true },
      ]),
    ).toBe('priority_asc');
  });
});
