import { useInfiniteQuery } from '@tanstack/vue-query';
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

/** 1 回に取る件数。backend の既定と同じにして、無指定でも同じ結果になるようにする。 */
export const ACTIVITIES_PAGE_SIZE = 20;

/**
 * タスクの操作履歴（作成・ステータス変更・担当者の増減など）。
 *
 * コメントとは別の口で、タスク詳細のアクティビティ欄に時系列で並べる。
 * 取得失敗は欄の中で倒し、タスク本体の表示には影響させない。
 *
 * 履歴は操作のたびに増えるので、開いた時点では先頭だけ取り、
 * 「もっと見る」で足す。全件取ると長く使われたタスクほどコストが伸びる。
 */
export function useTaskActivities(params: {
  tenantId: MaybeRefOrGetter<string | null | undefined>;
  projectId: MaybeRefOrGetter<string | null | undefined>;
  taskId: MaybeRefOrGetter<string | null | undefined>;
}) {
  const tenantId = computed(() => toValue(params.tenantId) ?? '');
  const projectId = computed(() => toValue(params.projectId) ?? '');
  const taskId = computed(() => String(toValue(params.taskId) ?? ''));

  const query = useInfiniteQuery(
    computed(() => {
      const path = { tenant_id: tenantId.value, project_id: projectId.value, id: taskId.value };
      return {
        // 更新系は ['get', ACTIVITIES_PATH] の prefix で invalidate する。
        // ページ番号はキーに入れない（infinite query が 1 キーで全ページを持つ）
        queryKey: ['get', ACTIVITIES_PATH, { params: { path } }],
        initialPageParam: 0,
        queryFn: async ({ pageParam, signal }: { pageParam: number; signal: AbortSignal }) => {
          const { data, error } = await fetchClient.GET(ACTIVITIES_PATH, {
            params: { path, query: { limit: ACTIVITIES_PAGE_SIZE, offset: pageParam } },
            signal,
          });
          if (error) throw error;
          return data;
        },
        getNextPageParam: (
          lastPage: { activities: ActivityItem[]; total: number },
          allPages: { activities: ActivityItem[] }[],
        ) => {
          const loaded = allPages.reduce((sum, page) => sum + page.activities.length, 0);
          // 最後のページが埋まっていない = 取り切った。total だけで判断すると、
          // 件数が変わったときに終わらない「もっと見る」が残る
          if (lastPage.activities.length < ACTIVITIES_PAGE_SIZE) return undefined;
          return loaded < lastPage.total ? loaded : undefined;
        },
        enabled: !!tenantId.value && !!projectId.value && !!taskId.value,
      };
    }),
  );

  return {
    activities: computed<ActivityItem[]>(
      () => query.data.value?.pages.flatMap((page) => page.activities) ?? [],
    ),
    activitiesLoading: computed(() => query.isLoading.value),
    activitiesError: computed(() => query.isError.value),
    /** まだ取れていない履歴があるか */
    hasMoreActivities: computed(() => !!query.hasNextPage.value),
    activitiesFetchingMore: computed(() => query.isFetchingNextPage.value),
    loadMoreActivities: () => void query.fetchNextPage(),
    refetchActivities: () => query.refetch(),
  };
}
