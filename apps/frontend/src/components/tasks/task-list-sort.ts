export type TaskListSortColumn = 'title' | 'assignee' | 'due_date' | 'priority';

export type TaskListSortEntry = {
  id: string;
  desc: boolean;
};

export type TaskListSortingState = TaskListSortEntry[];

export type TaskListSortColumnOption = {
  id: TaskListSortColumn;
  label: string;
  ascendingLabel: string;
  descendingLabel: string;
};

export const TASK_LIST_SORT_COLUMNS: readonly TaskListSortColumnOption[] = [
  {
    id: 'title',
    label: 'タスク',
    ascendingLabel: '名前の昇順',
    descendingLabel: '名前の降順',
  },
  {
    id: 'assignee',
    label: '担当',
    ascendingLabel: '担当者名の昇順',
    descendingLabel: '担当者名の降順',
  },
  {
    id: 'due_date',
    label: '期限',
    ascendingLabel: '期限が近い順',
    descendingLabel: '期限が遠い順',
  },
  {
    id: 'priority',
    label: '優先度',
    ascendingLabel: '優先度が高い順',
    descendingLabel: '優先度が低い順',
  },
] as const;

const API_SORT_BY_COLUMN: Record<TaskListSortColumn, string> = {
  title: 'title',
  assignee: 'assignee',
  due_date: 'deadline',
  priority: 'priority',
};

const SORTABLE_COLUMNS = new Set<TaskListSortColumn>(
  TASK_LIST_SORT_COLUMNS.map((column) => column.id),
);

export function activeTaskListSort(
  sorting: readonly TaskListSortEntry[],
): { id: TaskListSortColumn; desc: boolean } | null {
  const first = sorting[0];
  if (!first || !SORTABLE_COLUMNS.has(first.id as TaskListSortColumn)) return null;
  return { id: first.id as TaskListSortColumn, desc: first.desc };
}

/** List API の既定順は作成日時の新しい順。解除時も明示してクエリキーを安定させる。 */
export function taskListApiSort(sorting: readonly TaskListSortEntry[]): string {
  const active = activeTaskListSort(sorting);
  if (!active) return 'created_at_desc';
  return `${API_SORT_BY_COLUMN[active.id]}_${active.desc ? 'desc' : 'asc'}`;
}
