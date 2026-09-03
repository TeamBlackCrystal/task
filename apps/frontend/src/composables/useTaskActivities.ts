import { useQuery } from '@tanstack/vue-query';
import { computed, toValue, type MaybeRefOrGetter } from 'vue';

import { fetchClient } from '@/lib/api-vue-query';
import type { components } from '@/generated/api';

/**
 * 履歴の口。更新系から invalidate するため公開する。
 *
 * 履歴は backend が操作のたびに積むので、こちら側の mutation が成功したら
 * 取り直さないと欄だけ古いまま残る。
 */
export const ACTIVITIES_PATH =
  '/v1/tenants/{tenant_id}/projects/{project_id}/tasks/{id}/activities' as const;

export type ActivityItem = components['schemas']['ActivityItem'];

/**
 * タスクの操作履歴（作成・ステータス変更・担当者の増減など）。
 *
 * コメントとは別の口で、タスク詳細のアクティビティ欄に時系列で並べる。
 * 取得失敗は欄の中で倒し、タスク本体の表示には影響させない。
 */
export function useTaskActivities(params: {
  tenantId: MaybeRefOrGetter<string | null | undefined>;
  projectId: MaybeRefOrGetter<string | null | undefined>;
  taskId: MaybeRefOrGetter<string | null | undefined>;
}) {
  const tenantId = computed(() => toValue(params.tenantId) ?? '');
  const projectId = computed(() => toValue(params.projectId) ?? '');
  const taskId = computed(() => String(toValue(params.taskId) ?? ''));

  const query = useQuery(
    computed(() => {
      const path = { tenant_id: tenantId.value, project_id: projectId.value, id: taskId.value };
      return {
        queryKey: ['get', ACTIVITIES_PATH, { params: { path } }],
        queryFn: async ({ signal }: { signal: AbortSignal }) => {
          const { data, error } = await fetchClient.GET(ACTIVITIES_PATH, {
            params: { path },
            signal,
          });
          if (error) throw error;
          return data;
        },
        enabled: !!tenantId.value && !!projectId.value && !!taskId.value,
      };
    }),
  );

  return {
    activities: computed<ActivityItem[]>(() => query.data.value?.activities ?? []),
    activitiesLoading: computed(() => query.isLoading.value),
    activitiesError: computed(() => query.isError.value),
    refetchActivities: () => query.refetch(),
  };
}
