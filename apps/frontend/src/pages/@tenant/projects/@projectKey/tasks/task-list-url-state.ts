import type { SortingState } from '@tanstack/vue-table';
import { computed, watch, type Ref } from 'vue';

/**
 * タスク一覧の「今どこを見ているか」を URL クエリで持つ。
 *
 * ページ番号・検索語・ラベル絞り込み・並び替えはコンポーネントの ref だけに載っていて、
 * リロードや詳細ページからの戻りで初期値に戻っていた。3 ページ目まで送ってタスクを開き、
 * 戻ると 1 ページ目から探し直しになる。
 *
 * 置き場所を URL にすると、リロードとブラウザの戻るの両方が同じ経路で復元される
 * （選択タスクの `?selected=` が既にこの形）。localStorage だと URL を共有したときに
 * 相手と違う画面になり、タブごとに別の状態を持てない。
 */
export type TaskListUrlState = {
  /** 1 始まり。URL に出るので人が読める形にする（内部の pageIndex は 0 始まり） */
  page: number;
  /** 確定済みの検索語（デバウンス後に投げているもの） */
  q: string;
  labelId: string | null;
  sorting: SortingState;
};

export const DEFAULT_TASK_LIST_URL_STATE: TaskListUrlState = {
  page: 1,
  q: '',
  labelId: null,
  sorting: [],
};

/**
 * 並び替えを受け付ける列 id。`+Page.vue` の `columns` のうち
 * `enableSorting: false` でないもの（`select` と `labels` は対象外）。
 *
 * **拒否リストではなく許可リストにする。** TanStack の `getColumn` は
 * `flatColumns.reduce((acc, c) => { acc[c.id] = c }, {})` という素のオブジェクトを引くため、
 * `Object.prototype` のメンバー名（`__proto__` / `constructor` / `toString` /
 * `valueOf` / `hasOwnProperty` …）を渡すと prototype 側の値が返り、
 * 「存在しない列」として弾かれない。`getSortedRowModel` は
 * `table.getColumn(sort.id)?.getCanSort()` を呼ぶので `?.` をすり抜けて
 * `getCanSort is not a function` で throw し、一覧の描画が止まる。
 * 一覧は `getSortedRowModel()` を渡し `manualSorting` を付けていないので、
 * URL の値がそのままクライアント側のソートに入る。
 */
const SORTABLE_COLUMN_IDS = new Set(['key', 'title', 'status', 'priority', 'assignee', 'due_date']);

/** `title:asc,priority:desc` 形式。TanStack の SortingState と 1:1。 */
function parseSorting(raw: string | undefined): SortingState {
  if (!raw) return [];
  return raw
    .split(',')
    .map((entry) => {
      const [id, direction] = entry.split(':');
      // 並び替えできない列・向きが asc/desc 以外は捨てる。細工されたクエリで
      // TanStack へ不正な state を渡さない
      if (!id || !SORTABLE_COLUMN_IDS.has(id)) return null;
      if (direction !== 'asc' && direction !== 'desc') return null;
      return { id, desc: direction === 'desc' };
    })
    .filter((entry): entry is { id: string; desc: boolean } => entry !== null);
}

function serializeSorting(sorting: SortingState): string {
  return sorting.map((sort) => `${sort.id}:${sort.desc ? 'desc' : 'asc'}`).join(',');
}

function parsePage(raw: string | undefined): number {
  if (!raw) return DEFAULT_TASK_LIST_URL_STATE.page;
  const parsed = Number(raw);
  // 小数・0 以下・数値でないものは 1 ページ目に倒す（offset が負になる要求を作らない）
  if (!Number.isInteger(parsed) || parsed < 1) return DEFAULT_TASK_LIST_URL_STATE.page;
  return parsed;
}

export function parseTaskListUrlState(
  search: Record<string, string | undefined> | undefined,
): TaskListUrlState {
  return {
    page: parsePage(search?.page),
    q: search?.q?.trim() ?? '',
    labelId: search?.label || null,
    sorting: parseSorting(search?.sort),
  };
}

/**
 * 現在の状態を URL へ書き戻す。既定値のキーは落として URL を短く保つ。
 *
 * `selected` など、この関数が扱わないクエリは触らない（選択タスクは別の関心）。
 */
export function applyTaskListUrlState(url: URL, state: TaskListUrlState): URL {
  const next = new URL(url.href);
  const entries: [string, string][] = [
    ['page', state.page > 1 ? String(state.page) : ''],
    ['q', state.q],
    ['label', state.labelId ?? ''],
    ['sort', serializeSorting(state.sorting)],
  ];
  for (const [key, value] of entries) {
    if (value) next.searchParams.set(key, value);
    else next.searchParams.delete(key);
  }
  return next;
}

/**
 * 総件数が分かった時点で、範囲外のページを最後のページへ丸める。
 *
 * URL に古い `page=9` が残ったまま件数が減っていると、空のページが出て
 * 「タスクが消えた」ように見える。0 件のときは 1 ページ目のまま（丸め先が無い）。
 */
export function clampPage(page: number, total: number, pageSize: number): number {
  if (total <= 0 || pageSize <= 0) return DEFAULT_TASK_LIST_URL_STATE.page;
  const lastPage = Math.ceil(total / pageSize);
  return Math.min(Math.max(page, 1), lastPage);
}

/**
 * 一覧の状態を URL へ同期し、件数が分かったら範囲外のページを丸める。
 *
 * ページ側に watch を直接置くと、状態が増えたときに書き漏らしても気づけない
 * （型検査も lint も通る）。同期の配線ごとここへ寄せてテストで固定する。
 */
export function useTaskListUrlSync(params: {
  selectedTaskId: Ref<string | null>;
  pagination: Ref<{ pageIndex: number; pageSize: number }>;
  submittedSearchQuery: Ref<string>;
  selectedLabelId: Ref<string | null>;
  sorting: Ref<SortingState>;
  taskTotal: Readonly<Ref<number>>;
  isSearchActive: Readonly<Ref<boolean>>;
}) {
  const { selectedTaskId, pagination, submittedSearchQuery, selectedLabelId, sorting } = params;

  const listState = computed<TaskListUrlState>(() => ({
    page: pagination.value.pageIndex + 1,
    q: submittedSearchQuery.value,
    labelId: selectedLabelId.value,
    sorting: sorting.value,
  }));

  watch([selectedTaskId, listState], ([selected, state]) => {
    if (typeof window === 'undefined') return;
    let url = new URL(window.location.href);
    if (selected) url.searchParams.set('selected', selected);
    else url.searchParams.delete('selected');
    url = applyTaskListUrlState(url, state);
    // history を汚さない: 戻るで一覧より前へ抜けられなくなるのを避ける
    window.history.replaceState(window.history.state, '', url);
  });

  // 検索中はページャを使わない（常に先頭から一定件数）ので丸めの対象外
  watch([params.taskTotal, params.isSearchActive], ([total, searching]) => {
    if (searching) return;
    const clamped = clampPage(pagination.value.pageIndex + 1, total, pagination.value.pageSize);
    if (clamped - 1 !== pagination.value.pageIndex) {
      pagination.value = { ...pagination.value, pageIndex: clamped - 1 };
    }
  });

  return { listState };
}
