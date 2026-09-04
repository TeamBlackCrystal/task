import { ref, watch, type Ref } from 'vue';
import type { PaginationState } from '@tanstack/vue-table';

export type TasksListQueryKeyParams = {
  params: {
    path: { tenant_id: string; project_id: string };
    query: { limit: number; offset: number; label_id?: string };
  };
};

type PreviousTasksQuery = {
  queryKey: readonly unknown[];
};

type LabelOption = { id: string };

export function buildTasksListQueryParams(
  tenantId: string,
  projectId: string,
  pagination: PaginationState,
  selectedLabelId: string | null,
): TasksListQueryKeyParams {
  return {
    params: {
      path: { tenant_id: tenantId, project_id: projectId },
      query: {
        limit: pagination.pageSize,
        offset: pagination.pageIndex * pagination.pageSize,
        label_id: selectedLabelId ?? undefined,
      },
    },
  };
}

export function taskListPlaceholderData<T>(
  previousData: T | undefined,
  previousQuery: PreviousTasksQuery | undefined,
  currentProjectId: string | null,
  selectedLabelId: string | null,
): T | undefined {
  const previousParams = previousQuery?.queryKey[2] as TasksListQueryKeyParams | undefined;
  const previousProjectId = previousParams?.params.path.project_id;
  const previousLabelId = previousParams?.params.query.label_id ?? null;
  if (
    previousProjectId &&
    currentProjectId &&
    previousProjectId === currentProjectId &&
    previousLabelId === selectedLabelId
  ) {
    return previousData;
  }
  return undefined;
}

export function useTaskLabelFilter(
  pagination: Ref<PaginationState>,
  projectKey: Readonly<Ref<string>>,
  /** URL から復元した初期値（`null` は「すべて」） */
  initialLabelId: string | null = null,
) {
  const selectedLabelId = ref<string | null>(initialLabelId);

  watch(selectedLabelId, () => {
    pagination.value = { ...pagination.value, pageIndex: 0 };
  });
  watch(projectKey, () => {
    selectedLabelId.value = null;
  });

  return { selectedLabelId };
}

/**
 * 一覧に無いラベルの選択を解除する。
 *
 * `projectLabels` は **未取得を `null`** で受ける。未取得と「取得できて 0 件」を
 * 同じ空配列で表すと、キャッシュから同期的に得られたときに非即時の watcher が
 * 初期値を検査せず、URL に残った削除済み・不正な `?label=` がそのまま効いて
 * 空の一覧を出し続ける。
 */
export function watchAvailableTaskLabels(
  selectedLabelId: Ref<string | null>,
  projectLabels: Readonly<Ref<readonly LabelOption[] | null>>,
) {
  return watch(
    projectLabels,
    (labels) => {
      // 未取得のあいだは待つ（0 件と区別する）
      if (labels === null) return;
      if (selectedLabelId.value && !labels.some((label) => label.id === selectedLabelId.value)) {
        selectedLabelId.value = null;
      }
    },
    // キャッシュ由来のラベル一覧でも初回に検査する
    { immediate: true },
  );
}
