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
) {
  const selectedLabelId = ref<string | null>(null);

  watch(selectedLabelId, () => {
    pagination.value = { ...pagination.value, pageIndex: 0 };
  });
  watch(projectKey, () => {
    selectedLabelId.value = null;
  });

  return { selectedLabelId };
}

export function watchAvailableTaskLabels(
  selectedLabelId: Ref<string | null>,
  projectLabels: Readonly<Ref<readonly LabelOption[]>>,
) {
  return watch(projectLabels, (labels) => {
    if (selectedLabelId.value && !labels.some((label) => label.id === selectedLabelId.value)) {
      selectedLabelId.value = null;
    }
  });
}
