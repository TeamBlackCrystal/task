<script setup lang="ts">
import { Loader2 } from '@lucide/vue';
import { computed } from 'vue';
import { usePageContext } from 'vike-vue/usePageContext';

import ReviewFindingsView from '@/components/reviews/ReviewFindingsView.vue';
import { useResolvedProjectId } from '@/composables/useResolvedProjectId';
import { useResolvedTenantId } from '@/composables/useResolvedTenantId';
import { useMeQuery } from '@/lib/api-vue-query';

const pageContext = usePageContext();
const tenantDisplayId = computed(() => String(pageContext.routeParams.tenant ?? ''));
const projectKey = computed(() => String(pageContext.routeParams.projectKey ?? ''));

/** 要約コメントのリンク（`?pr=618`）から来たときは、その PR を開く。 */
const initialPr = computed(() => {
  const search = (pageContext as { urlParsed?: { search?: Record<string, string> } } | undefined)
    ?.urlParsed?.search;
  const raw = Number(search?.pr);
  return Number.isInteger(raw) && raw > 0 ? raw : null;
});

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
  isError: isProjectError,
} = useResolvedProjectId(tenantId, projectKey);

const meQuery = useMeQuery();

const isLoading = computed(
  () => isTenantResolving.value || isProjectResolving.value || meQuery.isPending.value,
);
const isError = computed(
  () => isTenantResolveError.value || isProjectError.value || meQuery.isError.value,
);
const isNotFound = computed(() => isTenantNotFound.value || isProjectNotFound.value);
</script>

<template>
  <div class="flex flex-col gap-6 px-4 pt-2 pb-10">
    <div v-if="isLoading" class="flex justify-center py-16">
      <Loader2 class="text-muted-foreground h-8 w-8 animate-spin" />
    </div>

    <p v-else-if="isError" class="text-destructive py-16 text-center text-sm">
      ページの読み込みに失敗しました
    </p>

    <p v-else-if="isNotFound" class="text-muted-foreground py-16 text-center text-sm">
      プロジェクトが見つかりません
    </p>

    <!--
      `:key` でプロジェクトごとに作り直す。

      vike-vue はクライアント遷移で同じ `+Page.vue` に解決される URL 間では
      コンポーネントを差し替えず patch するため、これが無いとクエリの引数
      （setup 時の値で固定される）も、選択中の PR や絞り込みも前のプロジェクトの
      ままになる。設定画面（`settings/+Page.vue`）と同じ扱い。
    -->
    <ReviewFindingsView
      v-else-if="tenantId && projectId && meQuery.data.value"
      :key="projectId"
      :tenant-id="tenantId"
      :tenant-slug="tenantDisplayId"
      :project-id="projectId"
      :project-key="projectKey"
      :viewer-id="meQuery.data.value.id"
      :initial-pr="initialPr"
    />
  </div>
</template>
