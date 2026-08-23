import { computed, ref, watch, type MaybeRefOrGetter, toValue } from 'vue';
import { useQuery, useQueryClient } from '@tanstack/vue-query';
import { navigate } from 'vike/client/router';

import type { EditableField } from '@/components/tasks/editable-field';
import { useResolvedProjectId } from '@/composables/useResolvedProjectId';
import { useResolvedTenantId } from '@/composables/useResolvedTenantId';
import { fetchClient, apiClient, TASK_SEARCH_PATH } from '@/lib/api-vue-query';
import { clampProgressPct, localDateInputToIso, taskListHref } from '@/lib/task-display';
import type { components } from '@/generated/api';

const GET_TASK_PATH = '/v1/tenants/{tenant_id}/projects/{project_id}/tasks/{id}' as const;
const LIST_STATUSES_PATH = '/v1/tenants/{tenant_id}/projects/{project_id}/statuses' as const;
const LIST_TASKS_PATH = '/v1/tenants/{tenant_id}/projects/{project_id}/tasks' as const;
const LIST_LABELS_PATH = '/v1/tenants/{tenant_id}/projects/{project_id}/labels' as const;

type TaskDetail = components['schemas']['TaskDetailResponse'];
type UpdateTaskRequest = components['schemas']['UpdateTaskRequest'];
export type MutatingField = EditableField | 'status_id' | 'labels';

/**
 * コードポイント順の文字列比較。
 * JS の `<` / `>` は UTF-16 コードユニット順で、サーバ（Rust の `String::cmp` =
 * コードポイント順）と非 BMP 文字（絵文字など）の並びがずれるため、
 * サロゲートペアを 1 文字として比較して両者を一致させる。
 */
function compareStrings(left: string, right: string) {
  const l = Array.from(left);
  const r = Array.from(right);
  const len = Math.min(l.length, r.length);
  for (let i = 0; i < len; i++) {
    const a = l[i].codePointAt(0)!;
    const b = r[i].codePointAt(0)!;
    if (a !== b) return a < b ? -1 : 1;
  }
  return l.length - r.length;
}

export interface UseTaskDetailParams {
  /** ルートの tenant セグメント（表示ID）。テナント UUID 解決に使う */
  tenantDisplayId: MaybeRefOrGetter<string>;
  /** プロジェクトの key。プロジェクト UUID 解決に使う */
  projectKey: MaybeRefOrGetter<string>;
  /** タスク識別子（URL と同じ seq key 形式。例: "ENG-42"）。空文字なら未取得 */
  taskId: MaybeRefOrGetter<string>;
  /**
   * 削除成功後に呼ばれる。省略時は一覧へ遷移する。
   * 分割ビューのペインでは「ペインを閉じる」を渡す。
   */
  onAfterDelete?: (listHref: string) => void;
  /**
   * フィールド保存がサーバへ確定した後に呼ばれる (mutateAsync 成功時)。
   * フルページ詳細はこれを description の保存に使い、+data.ts (サーバの
   * renderDescription) を再実行して描画済み HTML を取り直す —— クライアントで
   * KFM を描画しないための取り直し導線。
   */
  onAfterFieldSaved?: (field: MutatingField) => void;
}

/**
 * タスク詳細（取得・楽観更新・各フィールド保存・削除）のロジック。
 * フルページ詳細（@taskId/+Page.vue）と一覧の分割ビュー右ペインの両方から使う。
 * 表示は {@link TaskDetailHub} に委譲し、この composable は状態と操作だけを返す。
 */
export function useTaskDetail(params: UseTaskDetailParams) {
  const tenantDisplayId = computed(() => String(toValue(params.tenantDisplayId) ?? ''));
  const projectKey = computed(() => String(toValue(params.projectKey) ?? ''));
  const taskId = computed(() => String(toValue(params.taskId) ?? ''));
  const onAfterDelete = params.onAfterDelete ?? ((href: string) => void navigate(href));

  const queryClient = useQueryClient();

  const {
    tenantId,
    isTenantNotFound,
    isResolving: isTenantResolving,
    isError: isTenantResolveError,
  } = useResolvedTenantId(tenantDisplayId);
  const {
    projectId,
    isProjectNotFound,
    isResolving: isProjectResolving,
    isError: isProjectResolveError,
  } = useResolvedProjectId(tenantId, projectKey);

  const statusError = ref<string | null>(null);
  const labelsError = ref<string | null>(null);
  const deleteError = ref<string | null>(null);
  const fieldErrors = ref<Partial<Record<EditableField, string>>>({});
  const selectedStatusId = ref('');
  const optimisticTask = ref<Partial<TaskDetail>>({});
  const pendingFieldRevisions = ref<Partial<Record<MutatingField, number>>>({});
  const appliedFieldRevisions: Partial<Record<MutatingField, number>> = {};
  let nextMutationRevision = 0;

  const taskQueryKey = computed(
    () =>
      [
        'get',
        GET_TASK_PATH,
        {
          params: {
            path: {
              tenant_id: tenantId.value!,
              project_id: projectId.value!,
              id: taskId.value,
            },
          },
        },
      ] as const,
  );

  const taskQuery = useQuery({
    queryKey: taskQueryKey,
    queryFn: async ({ signal }) => {
      const { data, error, response } = await fetchClient.GET(GET_TASK_PATH, {
        params: {
          path: {
            tenant_id: tenantId.value!,
            project_id: projectId.value!,
            id: taskId.value,
          },
        },
        signal,
      });
      if (response.status === 404) return null;
      if (error) throw error;
      return data;
    },
    enabled: computed(() => !!tenantId.value && !!projectId.value && !!taskId.value),
  });

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

  watch(
    () => taskQuery.data.value?.status_id,
    (statusId) => {
      if (statusId) selectedStatusId.value = statusId;
    },
    { immediate: true },
  );

  const displayTask = computed(() => {
    const base = taskQuery.data.value;
    if (!base) return null;
    return { ...base, ...optimisticTask.value };
  });

  const fieldUpdating = computed(() => {
    const pending = pendingFieldRevisions.value;
    return {
      title: pending.title !== undefined,
      description: pending.description !== undefined,
      progress_pct: pending.progress_pct !== undefined,
      soft_deadline: pending.soft_deadline !== undefined,
      hard_deadline: pending.hard_deadline !== undefined,
    };
  });

  const statusUpdating = computed(() => pendingFieldRevisions.value.status_id !== undefined);
  const labelsUpdating = computed(() => pendingFieldRevisions.value.labels !== undefined);

  const updateTaskMutation = apiClient.useMutation('put', GET_TASK_PATH);

  const listHref = computed(() => taskListHref(tenantDisplayId.value, projectKey.value));

  /** 通常一覧と検索結果の両方を古い内容のまま残さないための invalidate。 */
  function invalidateTaskListCaches() {
    return Promise.all([
      queryClient.invalidateQueries({ queryKey: ['get', LIST_TASKS_PATH] }),
      queryClient.invalidateQueries({ queryKey: ['get', TASK_SEARCH_PATH] }),
    ]);
  }

  const deleteTaskMutation = apiClient.useMutation('delete', GET_TASK_PATH, {
    onSuccess: async () => {
      deleteError.value = null;
      queryClient.removeQueries({ queryKey: taskQueryKey.value, exact: true });
      await invalidateTaskListCaches();
      onAfterDelete(listHref.value);
    },
    onError: () => {
      deleteError.value = 'タスクの削除に失敗しました';
    },
  });

  function rollbackOptimistic(field: MutatingField, revision: number) {
    if (pendingFieldRevisions.value[field] !== revision) return;

    const nextOptimistic = { ...optimisticTask.value };
    delete nextOptimistic[field];
    optimisticTask.value = nextOptimistic;
    const nextPending = { ...pendingFieldRevisions.value };
    delete nextPending[field];
    pendingFieldRevisions.value = nextPending;

    if (field === 'status_id') {
      statusError.value = 'ステータスの更新に失敗しました';
      const currentStatusId = taskQuery.data.value?.status_id;
      if (currentStatusId) selectedStatusId.value = currentStatusId;
      return;
    }
    if (field === 'labels') {
      labelsError.value = 'ラベルの更新に失敗しました';
      return;
    }
    fieldErrors.value = {
      ...fieldErrors.value,
      [field]: '更新に失敗しました',
    };
  }

  function applyMutationSuccess(
    field: MutatingField,
    revision: number,
    data: TaskDetail,
    // mutation 開始時点の key。ペイン切替後の完了でも対象タスクのキャッシュに書くための固定値
    queryKey: typeof taskQueryKey.value,
  ) {
    const appliedRevision = appliedFieldRevisions[field] ?? 0;
    if (revision > appliedRevision) {
      appliedFieldRevisions[field] = revision;
      queryClient.setQueryData<TaskDetail | null>(queryKey, (current) =>
        current ? { ...current, [field]: data[field] } : data,
      );
    }

    if (pendingFieldRevisions.value[field] !== revision) return;

    const nextOptimistic = { ...optimisticTask.value };
    delete nextOptimistic[field];
    optimisticTask.value = nextOptimistic;
    const nextPending = { ...pendingFieldRevisions.value };
    delete nextPending[field];
    pendingFieldRevisions.value = nextPending;

    if (field === 'status_id') {
      statusError.value = null;
      if (data.status_id) selectedStatusId.value = data.status_id;
    } else if (field === 'labels') {
      labelsError.value = null;
    } else {
      fieldErrors.value = { ...fieldErrors.value, [field]: undefined };
    }
  }

  function mutateTask(
    body: UpdateTaskRequest,
    optimistic: Partial<TaskDetail>,
    field: MutatingField,
  ) {
    if (!tenantId.value || !projectId.value || !taskId.value) return;

    const revision = ++nextMutationRevision;
    optimisticTask.value = { ...optimisticTask.value, ...optimistic };
    pendingFieldRevisions.value = { ...pendingFieldRevisions.value, [field]: revision };
    if (field === 'status_id') statusError.value = null;
    else if (field === 'labels') labelsError.value = null;
    else fieldErrors.value = { ...fieldErrors.value, [field]: undefined };

    // mutate() のコールバックは observer の unmount（分割ビューのペイン切替）で
    // 破棄されるため、unmount 後も完走する mutateAsync の Promise 側で
    // キャッシュ更新と invalidate を行う。query key も開始時点で固定する。
    const queryKey = taskQueryKey.value;
    updateTaskMutation
      .mutateAsync({
        params: {
          path: {
            tenant_id: tenantId.value,
            project_id: projectId.value,
            id: taskId.value,
          },
        },
        body,
      })
      .then((data: TaskDetail) => {
        applyMutationSuccess(field, revision, data, queryKey);
        void invalidateTaskListCaches();
        params.onAfterFieldSaved?.(field);
      })
      .catch(() => {
        rollbackOptimistic(field, revision);
        // ラベルは task.labels 全量を送るため、削除済みラベルが混ざると 400 になる。
        // タスクとラベル一覧を再取得して古いキャッシュを解消し、再操作できる状態に戻す
        if (field === 'labels') {
          void queryClient.invalidateQueries({ queryKey });
          void queryClient.invalidateQueries({ queryKey: ['get', LIST_LABELS_PATH] });
        }
      });
  }

  function onStatusChange(nextStatusId: string) {
    if (!taskQuery.data.value) return;
    if (nextStatusId === taskQuery.data.value.status_id) return;

    selectedStatusId.value = nextStatusId;
    mutateTask({ status_id: nextStatusId }, { status_id: nextStatusId }, 'status_id');
  }

  function onSaveTitle(value: string) {
    const current = taskQuery.data.value;
    if (!current || value === current.title) return;
    mutateTask({ title: value }, { title: value }, 'title');
  }

  function onSaveDescription(value: string | null) {
    const current = taskQuery.data.value;
    if (!current) return;
    const normalized = value?.trim() ?? '';
    const currentDescription = current.description ?? '';
    if (normalized === currentDescription) return;

    const body: UpdateTaskRequest = normalized
      ? { description: normalized }
      : { clear_description: true };
    mutateTask(body, { description: normalized || null }, 'description');
  }

  function onSaveProgressPct(value: number) {
    const current = taskQuery.data.value;
    if (!current) return;
    const next = clampProgressPct(value);
    if (next === current.progress_pct) return;
    mutateTask({ progress_pct: next }, { progress_pct: next }, 'progress_pct');
  }

  function onSaveSoftDeadline(value: string | null) {
    const current = taskQuery.data.value;
    if (!current) return;

    if (!value) {
      if (!current.soft_deadline) return;
      mutateTask({ clear_soft_deadline: true }, { soft_deadline: null }, 'soft_deadline');
      return;
    }

    const iso = localDateInputToIso(value);
    if (iso === current.soft_deadline) return;
    mutateTask({ soft_deadline: iso }, { soft_deadline: iso }, 'soft_deadline');
  }

  function onSaveHardDeadline(value: string | null) {
    const current = taskQuery.data.value;
    if (!current) return;

    if (!value) {
      if (!current.hard_deadline) return;
      mutateTask({ clear_hard_deadline: true }, { hard_deadline: null }, 'hard_deadline');
      return;
    }

    const iso = localDateInputToIso(value);
    if (iso === current.hard_deadline) return;
    mutateTask({ hard_deadline: iso }, { hard_deadline: iso }, 'hard_deadline');
  }

  function onSaveLabels(labelIds: string[]) {
    const current = taskQuery.data.value;
    if (!current) return;
    const currentIds = current.labels.map((label) => label.id).sort();
    const nextIds = [...labelIds].sort();
    if (currentIds.length === nextIds.length && currentIds.every((id, i) => id === nextIds[i])) {
      return;
    }
    // 楽観値の出所は送信集合と揃える。projectLabels は task.labels より古いことがあるため、
    // 一覧だけを見ると既に付いているラベルが楽観描画から落ちて一瞬消えて見える。
    // 一覧を優先しつつ（名前の変更は一覧側が新しい）、欠けている分は task.labels で補う
    const known = new Map(current.labels.map((label) => [label.id, label]));
    for (const label of labelsQuery.data.value ?? []) {
      known.set(label.id, label);
    }
    // サーバレスポンスと同じ名前順（同名時は ID 順）に揃える
    const chosen = labelIds
      .map((id) => known.get(id))
      .filter((label) => label !== undefined)
      .sort(
        (left, right) => compareStrings(left.name, right.name) || compareStrings(left.id, right.id),
      );
    mutateTask({ label_ids: labelIds }, { labels: chosen }, 'labels');
  }

  function confirmDelete() {
    if (!tenantId.value || !projectId.value || !taskId.value) return;
    deleteError.value = null;
    deleteTaskMutation.mutate({
      params: {
        path: {
          tenant_id: tenantId.value,
          project_id: projectId.value,
          id: taskId.value,
        },
      },
    });
  }

  const isLoading = computed(
    () =>
      isTenantResolving.value ||
      isProjectResolving.value ||
      taskQuery.isLoading.value ||
      statusesQuery.isLoading.value,
  );

  const isError = computed(
    () =>
      isTenantResolveError.value ||
      isProjectResolveError.value ||
      taskQuery.isError.value ||
      statusesQuery.isError.value,
  );

  const isNotFound = computed(
    () =>
      isTenantNotFound.value ||
      isProjectNotFound.value ||
      (taskQuery.isSuccess.value && taskQuery.data.value === null),
  );

  return {
    // 解決済み ID（消費側で必要になれば使う）
    tenantId,
    projectId,
    // Hub バインド用
    displayTask,
    statuses: computed(() => statusesQuery.data.value ?? []),
    projectLabels: computed(() => labelsQuery.data.value ?? []),
    projectLabelsLoading: computed(() => labelsQuery.isLoading.value),
    projectLabelsError: computed(() => labelsQuery.isError.value),
    selectedStatusId,
    statusUpdating,
    statusError,
    labelsUpdating,
    labelsError,
    fieldUpdating,
    fieldErrors,
    isLoading,
    isNotFound,
    isError,
    // 操作
    onStatusChange,
    onSaveTitle,
    onSaveDescription,
    onSaveProgressPct,
    onSaveSoftDeadline,
    onSaveHardDeadline,
    onSaveLabels,
    // 削除
    deleteError,
    deletePending: computed(() => deleteTaskMutation.isPending.value),
    confirmDelete,
    // ナビゲーション
    listHref,
  };
}
