<script setup lang="ts">
import { Loader2, Search, Signal, SignalHigh, SignalLow, SignalMedium, X } from '@lucide/vue';
import type { LucideIcon } from '@lucide/vue';
import type {
  ColumnDef,
  ColumnFiltersState,
  PaginationState,
  SortingState,
  VisibilityState,
} from '@tanstack/vue-table';
import {
  FlexRender,
  getCoreRowModel,
  getFilteredRowModel,
  getSortedRowModel,
  useVueTable,
} from '@tanstack/vue-table';
import { PhCaretDown, PhCaretUp, PhCaretUpDown } from '@phosphor-icons/vue';
import { computed, h, onUnmounted, ref, watch } from 'vue';
import type { Column } from '@tanstack/vue-table';
import { useQuery, keepPreviousData } from '@tanstack/vue-query';
import { navigate } from 'vike/client/router';
import { usePageContext } from 'vike-vue/usePageContext';
import { useMediaQuery } from '@vueuse/core';

import { valueUpdater } from '@/components/ui/table/utils';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Input } from '@/components/ui/input';
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from '@/components/ui/resizable';
import TaskDetailPane from '@/components/tasks/TaskDetailPane.vue';
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import AvatarGroup from '@/components/AvatarGroup.vue';
import CreateTaskDialog from '@/components/tasks/CreateTaskDialog.vue';
import TaskTitleLink from '@/components/tasks/TaskTitleLink.vue';
import { useResolvedProjectId } from '@/composables/useResolvedProjectId';
import { useResolvedTenantId } from '@/composables/useResolvedTenantId';
import { fetchClient, taskSearchQueryOptions } from '@/lib/api-vue-query';
import { formatDeadline, taskDetailHref, taskSeqKey } from '@/lib/task-display';
import type { components } from '@/generated/api';
import {
  buildTasksListQueryParams,
  taskListPlaceholderData,
  useTaskLabelFilter,
  watchAvailableTaskLabels,
} from './task-list-label-filter';
import { shouldActivateRow, shouldOpenRowInNewTab } from './task-list-row-activate';
import { parseTaskListUrlState, useTaskListUrlSync } from './task-list-url-state';

// ---- 定数 ----
const LIST_TASKS_PATH = '/v1/tenants/{tenant_id}/projects/{project_id}/tasks' as const;
const LIST_STATUSES_PATH = '/v1/tenants/{tenant_id}/projects/{project_id}/statuses' as const;
const LIST_LABELS_PATH = '/v1/tenants/{tenant_id}/projects/{project_id}/labels' as const;
const LIST_MEMBERS_PATH = '/v1/tenants/{tenant_id}/projects/{project_id}/members' as const;
const TASKS_PAGE_SIZE = 20;
const SEARCH_PAGE_SIZE = 50;
const SEARCH_DEBOUNCE_MS = 300;

type TaskSearchQueryKeyParams = {
  params?: {
    path?: { tenant_id?: string; project_id?: string };
  };
};

// ---- 型定義 ----
type ApiPriority = components['schemas']['TaskPriority'];
type UserSummary = components['schemas']['UserSummary'];
type TaskLabel = components['schemas']['LabelResponse'];

interface TaskRow {
  id: string;
  seq_id: number;
  project_key: string;
  title: string;
  status: { id: string; name: string; color: string };
  priority: ApiPriority;
  assignees: UserSummary[];
  labels: TaskLabel[];
  due_date?: string;
}

// ---- ページコンテキスト ----
const pageContext = usePageContext();
const tenantDisplayId = computed(() => String(pageContext.routeParams.tenant ?? ''));
const {
  tenantId,
  isTenantNotFound,
  isResolving: isTenantResolving,
  isError: isTenantResolveError,
} = useResolvedTenantId(tenantDisplayId);
const projectKey = computed(() => String(pageContext.routeParams.projectKey ?? ''));
const {
  projectId,
  isProjectNotFound,
  isResolving: isProjectResolving,
  isError: isProjectResolveError,
} = useResolvedProjectId(tenantId, projectKey);

// ---- 分割ビュー: 選択タスクと詳細ペイン ----
// selectedTaskId は URL/詳細ページと同形の seq key（例: "ENG-42"）を保持する。
// これにより詳細クエリのキャッシュがフルページ詳細（@taskId）と共有される。
const selectedTaskId = ref<string | null>(null);

// 広い画面でのみ inline 分割を出す。狭い画面は従来どおり詳細ページへ遷移させる。
const canInline = useMediaQuery('(min-width: 1024px)');
const showDetail = computed(() => canInline.value && !!selectedTaskId.value);

// 一覧の状態（選択・ページ・検索語・ラベル・並び替え）を URL クエリから読む
// （クライアントのみ）。復元先の ref はこの後で定義するので、値だけ先に取る。
const initialUrlSearch = import.meta.env.SSR
  ? undefined
  : (pageContext as { urlParsed?: { search?: Record<string, string> } } | undefined)?.urlParsed
      ?.search;
const initialListState = parseTaskListUrlState(initialUrlSearch);
if (!import.meta.env.SSR) {
  const initialSelected = initialUrlSearch?.selected;
  if (initialSelected) selectedTaskId.value = initialSelected;
}

// プロジェクト切替時は選択を解除する（別プロジェクトのタスクを指したままにしない）。
watch(projectKey, () => {
  selectedTaskId.value = null;
});

/**
 * 行のクリック。広い画面では右ペインで開き、狭い画面は詳細ページへ送る。
 *
 * **判定は描画時ではなくクリック時に読む。** `useMediaQuery` は
 * `useSupported`（内部で `useMounted`）に依存するためマウント前は必ず false で、
 * マウント後に true へ変わる。この値を列定義の `cell` へ焼き込むと、TanStack の
 * `FlexRender` がセルを描き直さず false のまま固まり、広い画面なのに詳細ページへ
 * 飛ぶ（右ペインは出ているのに一覧のクリックだけ遷移する形で本番で発生した）。
 */
function onSelectRow(seqId: number) {
  if (!canInline.value) {
    void navigate(taskDetailHref(tenantDisplayId.value, projectKey.value, seqId));
    return;
  }
  selectedTaskId.value = taskSeqKey(projectKey.value, seqId);
}

/** 行のどこを押しても詳細へ入れるようにする（判定は task-list-row-activate に切り出し）。 */
function onRowActivate(event: MouseEvent, seqId: number) {
  // 修飾キー付きは別タブ。分割ビューでも「別タブで開く」を優先する
  if (openRowInNewTab(event, seqId)) return;
  if (!shouldActivateRow(event)) return;
  // 右ペインに出すか詳細ページへ送るかは onSelectRow に任せる。ここで同じ分岐を
  // 書くと、タイトルの `a` を押した経路と行を押した経路で判定が二重になる
  onSelectRow(seqId);
}

/** 中クリックは click ではなく auxclick で来る。 */
function onRowAuxClick(event: MouseEvent, seqId: number) {
  openRowInNewTab(event, seqId);
}

/**
 * 行を別タブで開けたら true。
 *
 * 行全体を覆う実リンクを外した代わりに、行側で新しいタブの操作を引き受ける。
 * `noopener` を付けて開いた先から元のページを触れないようにする。
 */
function openRowInNewTab(event: MouseEvent, seqId: number): boolean {
  if (!shouldOpenRowInNewTab(event)) return false;
  event.preventDefault();
  window.open(taskDetailHref(tenantDisplayId.value, projectKey.value, seqId), '_blank', 'noopener');
  return true;
}

function closeDetail() {
  selectedTaskId.value = null;
}

function isRowActive(seqId: number) {
  return !!selectedTaskId.value && taskSeqKey(projectKey.value, seqId) === selectedTaskId.value;
}

// ---- サーバー側検索 ----
const searchInput = ref(initialListState.q);
const submittedSearchQuery = ref(initialListState.q);
let searchDebounceTimer: ReturnType<typeof setTimeout> | undefined;

function updateSubmittedSearchQuery() {
  submittedSearchQuery.value = searchInput.value.trim();
}

function scheduleSearch(value: string | number) {
  if (searchDebounceTimer) clearTimeout(searchDebounceTimer);
  searchInput.value = String(value);
  if (!searchInput.value.trim()) {
    submittedSearchQuery.value = '';
    return;
  }
  searchDebounceTimer = setTimeout(updateSubmittedSearchQuery, SEARCH_DEBOUNCE_MS);
}

function submitSearch() {
  if (searchDebounceTimer) clearTimeout(searchDebounceTimer);
  updateSubmittedSearchQuery();
}

function clearSearch() {
  if (searchDebounceTimer) clearTimeout(searchDebounceTimer);
  searchInput.value = '';
  submittedSearchQuery.value = '';
}

onUnmounted(() => {
  if (searchDebounceTimer) clearTimeout(searchDebounceTimer);
});

const taskSearchQuery = useQuery(
  computed(() => ({
    ...taskSearchQueryOptions(
      tenantId.value ?? '',
      projectId.value ?? '',
      submittedSearchQuery.value,
      { limit: SEARCH_PAGE_SIZE, offset: 0 },
    ),
    enabled: !!tenantId.value && !!projectId.value && !!submittedSearchQuery.value,
    placeholderData: (previousData, previousQuery) => {
      const previousParams = previousQuery?.queryKey[2] as TaskSearchQueryKeyParams | undefined;
      const previousPath = previousParams?.params?.path;
      if (
        previousPath?.tenant_id === tenantId.value &&
        previousPath.project_id === projectId.value
      ) {
        return keepPreviousData(previousData);
      }
      return undefined;
    },
  })),
);

const isSearchActive = computed(() => !!submittedSearchQuery.value);

// ---- サーバーサイドページネーション ----
const pagination = ref<PaginationState>({
  // URL は 1 始まり（人が読む値）、TanStack は 0 始まり
  pageIndex: initialListState.page - 1,
  pageSize: TASKS_PAGE_SIZE,
});

// プロジェクト切替時は先頭ページへ戻す
watch(projectKey, () => {
  pagination.value = { ...pagination.value, pageIndex: 0 };
});

// ---- ラベルフィルタ ----
// null は「すべて」。切り替え時は先頭ページへ戻す
const { selectedLabelId } = useTaskLabelFilter(pagination, projectKey, initialListState.labelId);

// ---- クエリ②: タスク一覧 ----
const tasksQuery = useQuery({
  queryKey: computed(() => [
    'get',
    LIST_TASKS_PATH,
    buildTasksListQueryParams(
      tenantId.value!,
      projectId.value!,
      pagination.value,
      selectedLabelId.value,
    ),
  ]),
  queryFn: async ({ signal }) => {
    const { data, error } = await fetchClient.GET(LIST_TASKS_PATH, {
      // query パラメータは openapi-typescript 7.13.0 が正しく operation レベルに生成する
      ...buildTasksListQueryParams(
        tenantId.value!,
        projectId.value!,
        pagination.value,
        selectedLabelId.value,
      ),
      signal,
    });
    if (error) throw error;
    return data;
  },
  enabled: computed(() => !!tenantId.value && !!projectId.value),
  placeholderData: (previousData, previousQuery) => {
    // ラベルフィルタが変わったときは旧条件のデータを見せない（ページング時のみ維持）
    return taskListPlaceholderData(
      previousData,
      previousQuery,
      projectId.value,
      selectedLabelId.value,
    );
  },
});

// 取得済みの総件数。未取得は null にして「0 件だった」と区別する
// （同じ 0 だと範囲外ページの丸めが走らない経路ができる）
const fetchedTaskTotal = computed(() => tasksQuery.data.value?.total ?? null);
/** 表示用。未取得は 0 件として出す */
const taskTotal = computed(() => fetchedTaskTotal.value ?? 0);
const isCreateDialogOpen = ref(false);

// ---- クエリ③: ステータス一覧 ----
const statusesQuery = useQuery({
  queryKey: computed(() => [
    'get',
    LIST_STATUSES_PATH,
    { params: { path: { tenant_id: tenantId.value!, project_id: projectId.value! } } },
  ]),
  queryFn: async ({ signal }) => {
    const { data, error } = await fetchClient.GET(LIST_STATUSES_PATH, {
      params: { path: { tenant_id: tenantId.value!, project_id: projectId.value! } },
      signal,
    });
    if (error) throw error;
    return data;
  },
  enabled: computed(() => !!tenantId.value && !!projectId.value),
});

// ---- クエリ④: ラベル一覧（フィルタ用） ----
const labelsQuery = useQuery({
  queryKey: computed(() => [
    'get',
    LIST_LABELS_PATH,
    { params: { path: { tenant_id: tenantId.value!, project_id: projectId.value! } } },
  ]),
  queryFn: async ({ signal }) => {
    const { data, error } = await fetchClient.GET(LIST_LABELS_PATH, {
      params: { path: { tenant_id: tenantId.value!, project_id: projectId.value! } },
      signal,
    });
    if (error) throw error;
    return data;
  },
  enabled: computed(() => !!tenantId.value && !!projectId.value),
});

// ラベルも未取得を null で区別する（同上）
const fetchedProjectLabels = computed(() => labelsQuery.data.value ?? null);
const projectLabels = computed(() => fetchedProjectLabels.value ?? []);
watchAvailableTaskLabels(selectedLabelId, fetchedProjectLabels);

// 作成時に担当者を選べるようにする。候補はプロジェクトのメンバー
const membersQuery = useQuery({
  queryKey: computed(() => [
    'get',
    LIST_MEMBERS_PATH,
    { params: { path: { tenant_id: tenantId.value!, project_id: projectId.value! } } },
  ]),
  queryFn: async ({ signal }) => {
    const { data, error } = await fetchClient.GET(LIST_MEMBERS_PATH, {
      params: { path: { tenant_id: tenantId.value!, project_id: projectId.value! } },
      signal,
    });
    if (error) throw error;
    return data;
  },
  enabled: computed(() => !!tenantId.value && !!projectId.value),
});
watchAvailableTaskLabels(selectedLabelId, fetchedProjectLabels);
const selectedLabelName = computed(
  () => projectLabels.value.find((label) => label.id === selectedLabelId.value)?.name ?? null,
);

/** status_id → { name, color } 解決用 Map */
const statusMap = computed(() => {
  const statuses = statusesQuery.data.value ?? [];
  return new Map(statuses.map((s) => [s.id, { name: s.name, color: s.color }]));
});

// ---- テーブルデータ構築 ----
const taskRows = computed<TaskRow[]>(() => {
  const tasks = tasksQuery.data.value?.tasks;
  const sMap = statusMap.value;
  if (!tasks) return [];

  return tasks.map((t) => {
    const status = sMap.get(t.status_id) ?? { name: t.status_id, color: '#94a3b8' };
    return {
      id: t.id,
      seq_id: t.seq_id,
      project_key: projectKey.value,
      title: t.title,
      status: { id: t.status_id, ...status },
      priority: t.priority,
      assignees: t.assignees.map((a) => a.user),
      labels: t.labels,
      due_date: t.soft_deadline ?? undefined,
    };
  });
});

/** 初回ローディング表示。isLoading を使い、初回のみスピナー表示とする。
 *  背景refetch中は古いデータを表示し続ける（isFetching だとrefetch毎にテーブルが
 *  スピナーに置き換わりちらつくため）。refetch中の表示を強化したい場合は別途
 *  インジケーターを追加すること。 */
const isInitialLoading = computed(
  () =>
    isTenantResolving.value ||
    isProjectResolving.value ||
    tasksQuery.isLoading.value ||
    statusesQuery.isLoading.value,
);

const isError = computed(
  () =>
    isTenantResolveError.value ||
    isProjectResolveError.value ||
    tasksQuery.isError.value ||
    statusesQuery.isError.value,
);

// ---- ヘルパー ----
const PRIORITY_ORDER: Record<ApiPriority, number> = {
  CriticalFire: 0,
  Critical: 1,
  High: 2,
  Medium: 3,
  Low: 4,
  Trivial: 5,
};

/** ソート可能な列ヘッダー: 矢印アイコン付きボタンを返す */
function sortableHeader(column: Column<TaskRow>, label: string) {
  const sorted = column.getIsSorted();
  const icon =
    sorted === 'asc'
      ? h(PhCaretUp, { class: 'ml-1 size-4' })
      : sorted === 'desc'
        ? h(PhCaretDown, { class: 'ml-1 size-4' })
        : h(PhCaretUpDown, { class: 'ml-1 size-4 opacity-40' });
  return h(
    Button,
    {
      variant: 'ghost',
      class: '-ml-3 h-8 text-xs font-medium',
      onClick: () => column.toggleSorting(sorted === 'asc'),
    },
    () => [label, icon],
  );
}

const PRIORITY_CONFIG: Record<ApiPriority, { label: string; color: string; icon: LucideIcon }> = {
  CriticalFire: { label: '緊急', color: '#dc2626', icon: Signal },
  Critical: { label: '重大', color: '#ef4444', icon: Signal },
  High: { label: '高', color: '#f97316', icon: SignalHigh },
  Medium: { label: '中', color: '#eab308', icon: SignalMedium },
  Low: { label: '低', color: '#6b7280', icon: SignalLow },
  Trivial: { label: '些細', color: '#9ca3af', icon: SignalLow },
};

function taskKey(task: TaskRow) {
  return `${task.project_key}-${task.seq_id}`;
}

type CreatedTask = components['schemas']['TaskDetailResponse'];

function onTaskCreated(task: CreatedTask) {
  isCreateDialogOpen.value = false;
  // 分割ビューが出せる画面では作成タスクを右ペインで開く。狭い画面は詳細ページへ遷移。
  if (canInline.value) {
    selectedTaskId.value = taskSeqKey(projectKey.value, task.seq_id);
    return;
  }
  void navigate(taskDetailHref(tenantDisplayId.value, projectKey.value, task.seq_id));
}

// ---- テーブル列定義 ----
const columns: ColumnDef<TaskRow>[] = [
  {
    id: 'select',
    header: ({ table }) =>
      h(Checkbox, {
        modelValue:
          table.getIsAllPageRowsSelected() ||
          (table.getIsSomePageRowsSelected() && 'indeterminate'),
        'onUpdate:modelValue': (value) => table.toggleAllPageRowsSelected(!!value),
        ariaLabel: 'Select all',
      }),
    cell: ({ row }) =>
      h(Checkbox, {
        modelValue: row.getIsSelected(),
        'onUpdate:modelValue': (value) => row.toggleSelected(!!value),
        ariaLabel: 'Select row',
      }),
    enableSorting: false,
    enableHiding: false,
  },
  {
    id: 'key',
    accessorFn: (row) => taskKey(row),
    header: ({ column }) => sortableHeader(column, 'ID'),
    cell: ({ row }) =>
      h(
        'span',
        { class: 'font-mono text-xs text-muted-foreground whitespace-nowrap' },
        taskKey(row.original),
      ),
  },
  {
    accessorKey: 'title',
    header: ({ column }) => sortableHeader(column, 'タイトル'),
    cell: ({ row }) => {
      const task = row.original;
      const pc = PRIORITY_CONFIG[task.priority];
      return h('div', { class: 'flex items-center gap-2 min-w-0' }, [
        h(pc.icon, { class: 'size-4 shrink-0', style: { color: pc.color } }),
        h(TaskTitleLink, {
          tenantDisplayId: tenantDisplayId.value,
          projectKey: projectKey.value,
          seqId: task.seq_id,
          title: task.title,
          onSelect: onSelectRow,
        }),
      ]);
    },
  },
  {
    id: 'status',
    accessorFn: (row) => row.status.name,
    header: ({ column }) => sortableHeader(column, 'ステータス'),
    cell: ({ row }) => {
      const s = row.original.status;
      return h(
        'span',
        {
          class:
            'inline-flex items-center rounded-full border px-2 py-0.5 text-xs font-medium whitespace-nowrap',
          style: {
            backgroundColor: s.color + '1a',
            borderColor: s.color + '66',
            color: s.color,
          },
        },
        s.name,
      );
    },
  },
  {
    id: 'priority',
    accessorFn: (row) => row.priority,
    sortingFn: (a, b) => PRIORITY_ORDER[a.original.priority] - PRIORITY_ORDER[b.original.priority],
    header: ({ column }) => sortableHeader(column, '優先度'),
    cell: ({ row }) => {
      const pc = PRIORITY_CONFIG[row.original.priority];
      return h(
        'span',
        {
          class: 'inline-flex items-center gap-1 text-xs whitespace-nowrap',
          style: { color: pc.color },
        },
        [h(pc.icon, { class: 'size-4' }), pc.label],
      );
    },
  },
  {
    id: 'labels',
    enableSorting: false,
    header: () => 'ラベル',
    cell: ({ row }) => {
      const labels = row.original.labels;
      if (labels.length === 0) {
        return h('span', { class: 'text-muted-foreground text-xs' }, '−');
      }
      return h(
        'div',
        { class: 'flex flex-wrap gap-1' },
        labels.map((label) =>
          h(
            'span',
            {
              key: label.id,
              class:
                'inline-flex items-center rounded-full border px-2 py-0.5 text-xs font-medium whitespace-nowrap',
              style: {
                backgroundColor: label.color + '1a',
                borderColor: label.color + '66',
                color: label.color,
              },
            },
            label.name,
          ),
        ),
      );
    },
  },
  {
    id: 'assignee',
    accessorFn: (row) => row.assignees[0]?.username ?? '',
    header: ({ column }) => sortableHeader(column, '担当者'),
    cell: ({ row }) => {
      const users = row.original.assignees;
      if (users.length === 0) {
        return h('span', { class: 'text-muted-foreground text-xs' }, '−');
      }
      return h(AvatarGroup, { users, maxDisplay: 3 });
    },
  },
  {
    id: 'due_date',
    accessorFn: (row) => row.due_date ?? '',
    header: ({ column }) => sortableHeader(column, '期限'),
    cell: ({ row }) => {
      const formatted = formatDeadline(row.original.due_date);
      if (!formatted) return h('span', { class: 'text-muted-foreground text-xs' }, '−');
      return h(
        'span',
        {
          class: [
            'text-xs whitespace-nowrap',
            formatted.overdue ? 'text-red-500 font-medium' : 'text-muted-foreground',
          ],
        },
        formatted.label,
      );
    },
  },
];

// ---- テーブル状態 ----
const sorting = ref<SortingState>(initialListState.sorting);
const columnFilters = ref<ColumnFiltersState>([]);
const columnVisibility = ref<VisibilityState>({});
const rowSelection = ref({});

// ---- URL 同期 ----
// 選択・ページ・検索語・ラベル・並び替えを URL へ書き戻し、件数が分かったら
// 範囲外のページを丸める（配線ごと task-list-url-state に寄せてテストしている）。
useTaskListUrlSync({
  selectedTaskId,
  pagination,
  submittedSearchQuery,
  selectedLabelId,
  sorting,
  taskTotal: fetchedTaskTotal,
  isSearchActive,
});

const table = useVueTable({
  get data() {
    return taskRows.value;
  },
  columns,
  getRowId: (row) => row.id,
  getCoreRowModel: getCoreRowModel(),
  getSortedRowModel: getSortedRowModel(),
  getFilteredRowModel: getFilteredRowModel(),
  manualPagination: true,
  get rowCount() {
    return taskTotal.value;
  },
  onSortingChange: (u) => valueUpdater(u, sorting),
  onColumnFiltersChange: (u) => valueUpdater(u, columnFilters),
  onColumnVisibilityChange: (u) => valueUpdater(u, columnVisibility),
  onRowSelectionChange: (u) => valueUpdater(u, rowSelection),
  onPaginationChange: (u) => valueUpdater(u, pagination),
  state: {
    get sorting() {
      return sorting.value;
    },
    get columnFilters() {
      return columnFilters.value;
    },
    get columnVisibility() {
      return columnVisibility.value;
    },
    get rowSelection() {
      return rowSelection.value;
    },
    get pagination() {
      return pagination.value;
    },
  },
});
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col gap-3">
    <!-- ローディング / エラー表示 -->
    <div v-if="isInitialLoading" class="flex justify-center py-8">
      <Loader2 class="h-6 w-6 animate-spin text-muted-foreground" />
    </div>

    <div v-else-if="isError" class="flex justify-center py-8 text-sm text-destructive">
      タスクの読み込みに失敗しました
    </div>

    <div
      v-else-if="isTenantNotFound"
      class="flex justify-center py-8 text-sm text-muted-foreground"
    >
      テナントが見つかりません
    </div>

    <div
      v-else-if="isProjectNotFound"
      class="flex justify-center py-8 text-sm text-muted-foreground"
    >
      プロジェクトが見つかりません
    </div>

    <!-- 一覧（左）＋ 詳細ペイン（右）の分割ビュー -->
    <ResizablePanelGroup
      v-else
      direction="horizontal"
      auto-save-id="tasks-split-view"
      class="min-h-0 flex-1"
    >
      <ResizablePanel :order="1" :min-size="30" class="min-w-0">
        <div class="flex h-full min-h-0 flex-col gap-3">
          <!-- サーバー側検索ツールバー -->
          <div class="flex items-center gap-2">
            <form
              class="flex w-full max-w-md items-center gap-2"
              role="search"
              @submit.prevent="submitSearch"
            >
              <div class="relative min-w-0 flex-1">
                <Search
                  class="pointer-events-none absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground"
                />
                <Input
                  type="search"
                  class="h-8 appearance-none pl-8 pr-8 text-sm [&::-webkit-search-cancel-button]:hidden"
                  placeholder="タスクを検索..."
                  aria-label="タスクを検索"
                  :model-value="searchInput"
                  @update:model-value="scheduleSearch"
                />
                <Button
                  v-if="searchInput"
                  type="button"
                  variant="ghost"
                  size="icon"
                  class="absolute right-0 top-0 size-8"
                  aria-label="検索をクリア"
                  @click="clearSearch"
                >
                  <X class="size-4" />
                </Button>
              </div>
              <Button
                type="submit"
                variant="outline"
                size="sm"
                class="h-8"
                :disabled="!searchInput.trim()"
              >
                <Loader2
                  v-if="taskSearchQuery.isFetching.value && isSearchActive"
                  class="mr-1.5 size-4 animate-spin"
                />
                <Search v-else class="mr-1.5 size-4" />
                検索
              </Button>
            </form>
            <Button size="sm" class="ml-auto h-8 text-xs" @click="isCreateDialogOpen = true">
              新規タスク
            </Button>
            <!-- ラベル取得失敗はタスク一覧をブロックせず、ツールバー内で再試行を出す -->
            <div
              v-if="!isSearchActive && labelsQuery.isError.value && !projectLabels.length"
              class="flex items-center gap-1.5 text-xs text-destructive"
            >
              <span>ラベルの取得に失敗しました</span>
              <Button
                variant="outline"
                size="sm"
                class="h-8 text-xs"
                @click="labelsQuery.refetch()"
              >
                再試行
              </Button>
            </div>
            <DropdownMenu v-if="!isSearchActive && projectLabels.length">
              <DropdownMenuTrigger as-child>
                <Button
                  variant="outline"
                  size="sm"
                  class="h-8 text-xs"
                  :class="selectedLabelId ? 'border-primary text-primary' : ''"
                >
                  {{ selectedLabelName ?? 'ラベル' }} <PhCaretDown class="ml-1 size-4" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuRadioGroup
                  :model-value="selectedLabelId ?? ''"
                  @update:model-value="(v) => (selectedLabelId = v ? String(v) : null)"
                >
                  <DropdownMenuRadioItem class="text-sm" value="">すべて</DropdownMenuRadioItem>
                  <DropdownMenuRadioItem
                    v-for="label in projectLabels"
                    :key="label.id"
                    class="text-sm"
                    :value="label.id"
                  >
                    <span
                      class="mr-1.5 inline-block size-2.5 shrink-0 rounded-full"
                      :style="{ backgroundColor: label.color }"
                      aria-hidden="true"
                    />
                    {{ label.name }}
                  </DropdownMenuRadioItem>
                </DropdownMenuRadioGroup>
              </DropdownMenuContent>
            </DropdownMenu>
            <DropdownMenu v-if="!isSearchActive">
              <DropdownMenuTrigger as-child>
                <Button variant="outline" size="sm" class="h-8 text-xs">
                  列 <PhCaretDown class="ml-1 size-4" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuCheckboxItem
                  v-for="col in table.getAllColumns().filter((c) => c.getCanHide())"
                  :key="col.id"
                  class="text-sm"
                  :model-value="col.getIsVisible()"
                  @update:model-value="(v) => col.toggleVisibility(!!v)"
                >
                  {{ col.id }}
                </DropdownMenuCheckboxItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>

          <CreateTaskDialog
            v-if="tenantId && projectId"
            v-model:open="isCreateDialogOpen"
            :tenant-id="tenantId"
            :project-id="projectId"
            :project-key="projectKey"
            :statuses="statusesQuery.data.value ?? []"
            :labels="labelsQuery.data.value"
            :labels-loading="labelsQuery.isLoading.value"
            :labels-error="labelsQuery.isError.value && !projectLabels.length"
            :members="membersQuery.data.value"
            :members-loading="membersQuery.isLoading.value"
            :members-error="membersQuery.isError.value"
            @created="onTaskCreated"
            @retry-labels="labelsQuery.refetch()"
          />

          <!-- スクロールするテーブル領域（ツールバーとページネーションは固定） -->
          <div class="min-h-0 flex-1 overflow-y-auto">
            <!-- 検索結果。API は検索ヒットの最小情報のみ返すため、虚偽の状態値は補完しない。 -->
            <div
              v-if="
                isSearchActive && taskSearchQuery.isLoading.value && !taskSearchQuery.data.value
              "
              class="flex justify-center py-8"
            >
              <Loader2 class="h-6 w-6 animate-spin text-muted-foreground" />
            </div>
            <div
              v-else-if="isSearchActive && taskSearchQuery.isError.value"
              class="flex flex-col items-center gap-2 py-8 text-sm text-destructive"
            >
              <span>検索に失敗しました</span>
              <Button variant="outline" size="sm" @click="taskSearchQuery.refetch()">再試行</Button>
            </div>
            <template v-else-if="isSearchActive">
              <div class="rounded-md border overflow-x-auto">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead class="h-9 px-3 text-xs">ID</TableHead>
                      <TableHead class="h-9 px-3 text-xs">タイトル</TableHead>
                      <TableHead class="h-9 px-3 text-xs">一致箇所</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    <TableRow
                      v-for="task in taskSearchQuery.data.value?.tasks ?? []"
                      :key="task.id"
                      class="h-10 cursor-pointer"
                      :class="isRowActive(task.seq_id) && 'bg-muted'"
                      @click="onRowActivate($event, task.seq_id)"
                      @auxclick="onRowAuxClick($event, task.seq_id)"
                    >
                      <TableCell class="px-3 py-1.5 font-mono text-xs text-muted-foreground">
                        {{ projectKey }}-{{ task.seq_id }}
                      </TableCell>
                      <TableCell class="px-3 py-1.5">
                        <TaskTitleLink
                          :tenant-display-id="tenantDisplayId"
                          :project-key="projectKey"
                          :seq-id="task.seq_id"
                          :title="task.title"
                          @select="onSelectRow"
                        />
                      </TableCell>
                      <!-- highlight は backend の ilike/tsvector/no-match 全経路で動的文字列を
                     html_escape 済み。唯一 backend が付与する <em> のみ HTML として描画する。 -->
                      <TableCell
                        class="max-w-md truncate px-3 py-1.5 text-xs text-muted-foreground"
                        v-html="task.highlight"
                      />
                    </TableRow>
                    <TableRow v-if="!taskSearchQuery.data.value?.tasks.length">
                      <TableCell
                        :colspan="3"
                        class="h-24 text-center text-sm text-muted-foreground"
                      >
                        検索結果がありません
                      </TableCell>
                    </TableRow>
                  </TableBody>
                </Table>
              </div>
              <div class="text-xs text-muted-foreground">
                上位 {{ taskSearchQuery.data.value?.tasks.length ?? 0 }} 件 / 全
                {{ taskSearchQuery.data.value?.total ?? 0 }} 件
              </div>
            </template>

            <!-- 通常一覧テーブル -->
            <div v-else class="rounded-md border overflow-x-auto">
              <Table>
                <TableHeader>
                  <TableRow v-for="hg in table.getHeaderGroups()" :key="hg.id">
                    <TableHead
                      v-for="header in hg.headers"
                      :key="header.id"
                      class="h-9 text-xs px-3"
                    >
                      <FlexRender
                        v-if="!header.isPlaceholder"
                        :render="header.column.columnDef.header"
                        :props="header.getContext()"
                      />
                    </TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  <template v-if="table.getRowModel().rows?.length">
                    <TableRow
                      v-for="row in table.getRowModel().rows"
                      :key="row.id"
                      :data-state="row.getIsSelected() && 'selected'"
                      class="h-10 cursor-pointer"
                      :class="isRowActive(row.original.seq_id) && 'bg-muted'"
                      @click="onRowActivate($event, row.original.seq_id)"
                      @auxclick="onRowAuxClick($event, row.original.seq_id)"
                    >
                      <TableCell
                        v-for="cell in row.getVisibleCells()"
                        :key="cell.id"
                        class="py-1.5 px-3"
                      >
                        <FlexRender
                          :render="cell.column.columnDef.cell"
                          :props="cell.getContext()"
                        />
                      </TableCell>
                    </TableRow>
                  </template>
                  <TableRow v-else>
                    <TableCell
                      :colspan="columns.length"
                      class="h-24 text-center text-sm text-muted-foreground"
                    >
                      タスクが見つかりません
                    </TableCell>
                  </TableRow>
                </TableBody>
              </Table>
            </div>
          </div>

          <!-- ページネーション（API total 連動のサーバーサイド） -->
          <div
            v-if="!isSearchActive"
            class="flex items-center justify-between text-xs text-muted-foreground"
          >
            <span>
              {{ table.getFilteredSelectedRowModel().rows.length }} / {{ taskTotal }} 件選択
            </span>
            <div class="flex items-center gap-2">
              <span>
                {{ taskTotal === 0 ? 0 : pagination.pageIndex * pagination.pageSize + 1 }}–{{
                  Math.min((pagination.pageIndex + 1) * pagination.pageSize, taskTotal)
                }}
                / {{ taskTotal }} 件
              </span>
              <div class="flex gap-1.5">
                <Button
                  variant="outline"
                  size="sm"
                  class="h-7 text-xs"
                  :disabled="!table.getCanPreviousPage()"
                  @click="table.previousPage()"
                  >前へ</Button
                >
                <Button
                  variant="outline"
                  size="sm"
                  class="h-7 text-xs"
                  :disabled="!table.getCanNextPage()"
                  @click="table.nextPage()"
                  >次へ</Button
                >
              </div>
            </div>
          </div>
        </div>
      </ResizablePanel>

      <template v-if="showDetail">
        <ResizableHandle with-handle />
        <ResizablePanel :order="2" :default-size="40" :min-size="26" class="min-w-0">
          <TaskDetailPane
            :key="selectedTaskId ?? ''"
            :tenant-display-id="tenantDisplayId"
            :project-key="projectKey"
            :task-id="selectedTaskId ?? ''"
            @close="closeDetail"
          />
        </ResizablePanel>
      </template>
    </ResizablePanelGroup>
  </div>
</template>
