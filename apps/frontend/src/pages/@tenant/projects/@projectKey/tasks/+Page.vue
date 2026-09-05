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
import { PhCaretDown, PhCaretUp, PhCaretUpDown, PhRows, PhTable } from '@phosphor-icons/vue';
import { computed, h, onUnmounted, ref, watch, type Component } from 'vue';
import type { Column } from '@tanstack/vue-table';
import { useQueries, useQuery, keepPreviousData } from '@tanstack/vue-query';
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
import { fetchClient, taskSearchQueryOptions, useAssignableUsersQuery } from '@/lib/api-vue-query';
import { formatDeadline, taskDetailHref, taskSeqKey } from '@/lib/task-display';
import type { components } from '@/generated/api';
import {
  buildTasksListQueryParams,
  taskListPlaceholderData,
  useTaskLabelFilter,
  watchAvailableTaskLabels,
} from './task-list-label-filter';
import { shouldActivateRow, shouldOpenRowInNewTab } from './task-list-row-activate';
import {
  parseTaskListUrlState,
  useTaskListUrlSync,
  type TaskListView,
} from './task-list-url-state';
import TaskGroupedList from '@/components/tasks/TaskGroupedList.vue';
import type { TaskGroup } from '@/components/tasks/task-grouped-columns';
import { nextGroupCursor, toTaskGroup } from '@/components/tasks/task-group-pages';
import TaskDetailOverlay from '@/components/tasks/TaskDetailOverlay.vue';
import { useTaskRowMutations } from '@/composables/useTaskRowMutations';

// ---- 定数 ----
const LIST_TASKS_PATH = '/v1/tenants/{tenant_id}/projects/{project_id}/tasks' as const;
const LIST_STATUSES_PATH = '/v1/tenants/{tenant_id}/projects/{project_id}/statuses' as const;
/** List 表示でステータスごとに最初に取る件数。「もっと見る」でこの単位ずつ増やす */
const GROUP_PAGE_SIZE = 20;
const LIST_LABELS_PATH = '/v1/tenants/{tenant_id}/projects/{project_id}/labels' as const;
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
// Table 表示だけが右ペインを出す。List 表示の詳細はオーバーレイ（overlayTaskSeqKey）。
const showDetail = computed(() => canInline.value && !!selectedTaskId.value && !isListView.value);

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
// ---- 表示形式（既定は List）----
const VIEW_OPTIONS: { value: TaskListView; label: string; icon: Component }[] = [
  { value: 'list', label: 'List', icon: PhRows },
  { value: 'table', label: 'Table', icon: PhTable },
];
const view = ref<TaskListView>(initialListState.view);
const isListView = computed(() => view.value === 'list');

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
/**
 * Table 表示の一覧（ページャつき）を使うか。
 *
 * List 表示はステータス別の `groupQueries` から作るので、この一覧は使わない。
 * 使わないまま取りに行くと、余分なリクエストが増えるだけでなく、ローディングと
 * エラーがこの query に紐付いているために List が出せない・List 全体が
 * エラー画面になる。検索中も表示に使わないので同じく止める。
 */
const usesTaskList = computed(() => !isListView.value && !isSearchActive.value);

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
  enabled: computed(() => !!tenantId.value && !!projectId.value && usesTaskList.value),
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

// ラベルも未取得を null で区別する（表示用は空配列へ畳む）
const fetchedProjectLabels = computed(() => labelsQuery.data.value ?? null);
const projectLabels = computed(() => fetchedProjectLabels.value ?? []);

// ---- クエリ④: 担当者候補（List 表示の行から割り当てる）----
const membersQuery = useAssignableUsersQuery(
  // 担当者の割り当ては List 表示でしか出さないので、Table のときは取りに行かない
  () => (isListView.value ? tenantId.value : null),
  () => (isListView.value ? projectId.value : null),
);

const projectMembers = computed(() => membersQuery.data.value ?? []);
// 候補の取得状態はピッカーへ渡す（取得中・失敗を「候補 0 人」と混ぜない）
const projectMembersState = computed(() => ({
  loading: membersQuery.isLoading.value,
  error: membersQuery.isError.value,
  onRetry: () => void membersQuery.refetch(),
}));

// ---- List 表示: ステータスごとの取得 ----
// 1 本の一覧クエリを画面側で仕分けると、グループの件数がページ内の件数になってしまう
// （「Todo 3」なのに実際は 40 件、など）。ステータスで絞った問い合わせを並べ、
// 件数はサーバの total を使う。
//
// 「もっと見る」は limit を伸ばすのではなくページを足す。limit を伸ばす方式だと
// サーバ側の上限（tasks.rs の `clamp(1, 200)`）に当たった時点で 201 件目以降へ
// 進めなくなり、押すたびに同じ 200 件を取り直すだけになる。ページ単位なら取得済みの
// ページがキャッシュに残るので、押した瞬間に行が消える問題も起きない。
//
// ページの継ぎ目は offset ではなくサーバの next_cursor で持つ。offset だと、
// 1 ページ目を読んだ後にタスクが 1 件でも他のステータスへ移ると後続の境界が詰まり、
// 境界のタスクがどのページにも現れない（重複は ID で落とせるが、欠落は戻らない）。
//
// 先頭は必ず null（カーソル無し）で、「もっと見る」のたびに最後のページの
// next_cursor を積む。
const groupCursors = ref<Record<string, (string | null)[]>>({});

// プロジェクトやラベル絞り込みを変えたら取得済みページを戻す（前の条件の分だけ残さない）。
// カーソルは絞り込み後の並びの中の位置なので、条件が変わったら意味を失う
watch([projectKey, selectedLabelId], () => {
  groupCursors.value = {};
});

const workflowStatuses = computed(() => statusesQuery.data.value ?? []);

/** (ステータス, カーソル) の組。useQueries は平らな配列しか取れないので、後で畳み直す。 */
const groupPageRequests = computed(() =>
  workflowStatuses.value.flatMap((status) =>
    (groupCursors.value[status.id] ?? [null]).map((cursor) => ({ statusId: status.id, cursor })),
  ),
);

const groupQueries = useQueries({
  queries: computed(() =>
    groupPageRequests.value.map(({ statusId, cursor }) => {
      const params = {
        params: {
          path: { tenant_id: tenantId.value!, project_id: projectId.value! },
          query: {
            status_id: statusId,
            label_id: selectedLabelId.value ?? undefined,
            limit: GROUP_PAGE_SIZE,
            cursor: cursor ?? undefined,
          },
        },
      };
      return {
        queryKey: ['get', LIST_TASKS_PATH, params],
        queryFn: async ({ signal }: { signal: AbortSignal }) => {
          const { data, error } = await fetchClient.GET(LIST_TASKS_PATH, { ...params, signal });
          if (error) throw error;
          return data;
        },
        enabled: !!tenantId.value && !!projectId.value && isListView.value && !isSearchActive.value,
      };
    }),
  ),
});

/** ステータス 1 つ分のページを、要求した順のまま取り出す。 */
function pagesOfStatus(statusId: string) {
  return groupPageRequests.value.map((request, index) =>
    request.statusId === statusId ? groupQueries.value[index] : null,
  );
}

const taskGroups = computed<TaskGroup[]>(() =>
  workflowStatuses.value.map((status) => toTaskGroup(status, pagesOfStatus(status.id))),
);

function loadMoreInGroup(statusId: string) {
  const cursor = nextGroupCursor(pagesOfStatus(statusId));
  // 取り切っている / まだ返ってきていないときは足さない。押しても増えないだけで、
  // 同じカーソルを 2 度積んで同じページを二重に並べるより安全
  if (cursor === null) return;
  const current = groupCursors.value[statusId] ?? [null];
  if (current.includes(cursor)) return;
  groupCursors.value = { ...groupCursors.value, [statusId]: [...current, cursor] };
}

// ---- List 表示: 行からの更新 ----
const rowMutations = useTaskRowMutations({
  tenantId: () => tenantId.value,
  projectId: () => projectId.value,
});
// ---- List 表示: 詳細のオーバーレイ ----
//
// 選択は表示形式によらず selectedTaskId に一本化し、List 表示のときだけ
// オーバーレイとして読み替える。ここを別の ref に分けると、検索結果のクリックや
// タスク作成のように selectedTaskId しか触らない経路が List 表示で無反応になる。
const overlayTaskSeqKey = computed(() =>
  isListView.value && canInline.value ? selectedTaskId.value : null,
);

/**
 * 閉じるアニメーションの間だけ、直前に開いていたタスクを保持する。
 *
 * 選択を null にした瞬間に v-if でオーバーレイごと消すと、Dialog の退場
 * アニメーションが再生されないまま画面から消える。
 */
const overlayRenderedTaskSeqKey = ref<string | null>(null);
let overlayCleanupTimer: ReturnType<typeof setTimeout> | undefined;

watch(
  overlayTaskSeqKey,
  (seqKey) => {
    if (overlayCleanupTimer) clearTimeout(overlayCleanupTimer);
    if (seqKey) {
      overlayRenderedTaskSeqKey.value = seqKey;
      return;
    }
    // Dialog の duration-200 に合わせて、閉じ切ってから外す
    overlayCleanupTimer = setTimeout(() => {
      overlayRenderedTaskSeqKey.value = null;
    }, 250);
  },
  // URL の ?selected= から復元した選択も拾う（初回は watch が走らないため）
  { immediate: true },
);

onUnmounted(() => {
  if (overlayCleanupTimer) clearTimeout(overlayCleanupTimer);
});

function openOverlay(taskId: string) {
  const task = taskGroups.value.flatMap((group) => group.tasks).find((t) => t.id === taskId);
  if (!task) return;
  onSelectRow(task.seq_id);
}
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
    statusesQuery.isLoading.value ||
    // Table 用一覧を使わない表示（List・検索）では、この query で画面を止めない
    (usesTaskList.value && tasksQuery.isLoading.value),
);

const isError = computed(
  () =>
    isTenantResolveError.value ||
    isProjectResolveError.value ||
    statusesQuery.isError.value ||
    (usesTaskList.value && tasksQuery.isError.value),
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
  view,
  pagination,
  submittedSearchQuery,
  selectedLabelId,
  sorting,
  taskTotal: fetchedTaskTotal,
  isPagerActive: usesTaskList,
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

          <!-- 表示形式の切り替え。既定は List -->
          <div class="flex items-center gap-1 border-b" role="tablist" aria-label="表示形式">
            <button
              v-for="option in VIEW_OPTIONS"
              :key="option.value"
              type="button"
              role="tab"
              :aria-selected="view === option.value"
              class="-mb-px flex items-center gap-1.5 border-b-2 px-2.5 py-1.5 text-sm transition-colors"
              :class="
                view === option.value
                  ? 'border-foreground font-semibold text-foreground'
                  : 'border-transparent text-muted-foreground hover:text-foreground'
              "
              @click="view = option.value"
            >
              <component :is="option.icon" class="size-4" />
              {{ option.label }}
            </button>
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

            <!-- List 表示: ステータスごとに並べる（既定） -->
            <TaskGroupedList
              v-else-if="isListView"
              :groups="taskGroups"
              :statuses="workflowStatuses"
              :project-labels="projectLabels"
              :members="projectMembers"
              :members-state="projectMembersState"
              :pending="rowMutations.pending.value"
              :errors="rowMutations.errors.value"
              :comment-pending-task-ids="rowMutations.commentPendingTaskIds.value"
              :create-errors="rowMutations.createErrors.value"
              @open="openOverlay"
              @more="loadMoreInGroup"
              @update:status="(task, statusId) => rowMutations.setStatus(task, statusId)"
              @update:priority="(task, priority) => rowMutations.setPriority(task, priority)"
              @update:soft-deadline="(task, iso) => rowMutations.setSoftDeadline(task, iso)"
              @toggle:assignee="
                (task, userId, checked) => rowMutations.toggleAssignee(task, userId, checked)
              "
              @toggle:label="
                (task, labelId, checked) => rowMutations.toggleLabel(task, labelId, checked)
              "
              :on-comment="(task, body) => rowMutations.addComment(task.id, body)"
              :creating-status-ids="rowMutations.creatingStatusIds.value"
              :on-create="rowMutations.createTask"
            />

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

          <!-- ページネーション（API total 連動のサーバーサイド）。
               List 表示はステータスごとに「もっと見る」で伸ばすのでページャは出さない -->
          <div
            v-if="!isSearchActive && !isListView"
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

    <!-- List 表示の詳細は分割ではなくオーバーレイ -->
    <TaskDetailOverlay
      v-if="overlayRenderedTaskSeqKey"
      :key="overlayRenderedTaskSeqKey"
      :open="!!overlayTaskSeqKey"
      :tenant-display-id="tenantDisplayId"
      :project-key="projectKey"
      :task-id="overlayRenderedTaskSeqKey"
      @update:open="
        (value) => {
          if (!value) selectedTaskId = null;
        }
      "
    />
  </div>
</template>
