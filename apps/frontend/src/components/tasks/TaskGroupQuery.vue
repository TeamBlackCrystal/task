<script setup lang="ts">
/**
 * ステータス 1 つ分のタスクを取ってくるだけの、描画しないコンポーネント。
 *
 * ページングを infinite query で持つために、ステータスごとにコンポーネントを分ける。
 * `useQueries` でページを 1 本ずつ別クエリにすると、後続ページのキーに取得時点の
 * カーソルが焼き付き、先頭ページが取り直されて中身が変わっても後続は古い鍵のまま引く
 * （境界のタスクがどのページにも出なくなる）。infinite query ならページを順に引き直し、
 * 鍵をそのつど前のページから採り直すので、window focus や外からの invalidate で
 * 取り直されても並びが繋がったままになる。
 *
 * 呼ぶ側はカーソルを一切持たない。「もっと見る」は `TaskGroup.loadMore()` を呼ぶ。
 */
import { computed, watch } from 'vue';
import { useInfiniteQuery } from '@tanstack/vue-query';

import { fetchClient } from '@/lib/api-vue-query';
import { toTaskGroup, type TaskGroupPage } from '@/components/tasks/task-group-pages';
import type { TaskGroup } from '@/components/tasks/task-grouped-columns';
import type { components } from '@/generated/api';

type StatusResponse = components['schemas']['ProjectStatusResponse'];

const LIST_TASKS_PATH = '/v1/tenants/{tenant_id}/projects/{project_id}/tasks' as const;

const props = defineProps<{
  status: StatusResponse;
  tenantId: string | null | undefined;
  projectId: string | null | undefined;
  labelId?: string | null;
  sort: string;
  pageSize: number;
  enabled: boolean;
}>();

const emit = defineEmits<{ (e: 'update:group', group: TaskGroup): void }>();

const query = useInfiniteQuery(
  computed(() => {
    const path = { tenant_id: props.tenantId!, project_id: props.projectId! };
    const query = {
      status_id: props.status.id,
      label_id: props.labelId ?? undefined,
      sort: props.sort,
      limit: props.pageSize,
    };
    return {
      // 更新系は ['get', LIST_TASKS_PATH] の prefix で invalidate する。
      // カーソルはキーに入れない（infinite query が 1 キーで全ページを持つ）
      queryKey: ['get', LIST_TASKS_PATH, { params: { path, query } }],
      // 先頭ページは cursor を付けない
      initialPageParam: null as string | null,
      queryFn: async ({ pageParam, signal }: { pageParam: string | null; signal: AbortSignal }) => {
        const { data, error } = await fetchClient.GET(LIST_TASKS_PATH, {
          params: { path, query: { ...query, cursor: pageParam ?? undefined } },
          signal,
        });
        if (error) throw error;
        return data;
      },
      getNextPageParam: (lastPage: { next_cursor?: string | null }) =>
        lastPage.next_cursor ?? undefined,
      enabled: props.enabled && !!props.tenantId && !!props.projectId,
    };
  }),
);

const group = computed(() =>
  toTaskGroup(props.status, {
    data: query.data.value as { pages: TaskGroupPage[] } | undefined,
    isLoading: query.isLoading.value,
    isFetchingNextPage: query.isFetchingNextPage.value,
    isError: query.isError.value,
    hasNextPage: query.hasNextPage.value,
    refetch: () => query.refetch(),
    fetchNextPage: () => query.fetchNextPage(),
  }),
);

watch(group, (value) => emit('update:group', value), { immediate: true });
</script>

<template>
  <!-- 取得だけを受け持つ。描画は TaskGroupedList 側 -->
</template>
