<script setup lang="ts">
import { Loader2 } from '@lucide/vue';
import { computed } from 'vue';
import { usePageContext } from 'vike-vue/usePageContext';

import ProjectSettingsView from '@/components/projects/ProjectSettingsView.vue';
import { useResolvedProjectId } from '@/composables/useResolvedProjectId';
import { useResolvedTenantId } from '@/composables/useResolvedTenantId';
import type { components } from '@/generated/api';

type ProjectResponse = components['schemas']['ProjectResponse'];

const pageContext = usePageContext();
const tenantDisplayId = computed(() => String(pageContext.routeParams.tenant ?? ''));
const projectKey = computed(() => String(pageContext.routeParams.projectKey ?? ''));

const {
  tenantId,
  isTenantNotFound,
  isResolving: isTenantResolving,
  isError: isTenantResolveError,
} = useResolvedTenantId(tenantDisplayId);

const {
  isProjectNotFound,
  isResolving: isProjectResolving,
  isError: isProjectError,
  projectsQuery,
} = useResolvedProjectId(tenantId, projectKey);

const project = computed(
  () => projectsQuery.data.value?.find((p: ProjectResponse) => p.key === projectKey.value) ?? null,
);

const isLoading = computed(() => isTenantResolving.value || isProjectResolving.value);
const isError = computed(() => isTenantResolveError.value || isProjectError.value);
const isNotFound = computed(() => isTenantNotFound.value || isProjectNotFound.value);
</script>

<template>
  <div class="flex flex-col gap-6 px-4 pb-10 pt-2">
    <div v-if="isLoading" class="flex justify-center py-16">
      <Loader2 class="h-8 w-8 animate-spin text-muted-foreground" />
    </div>

    <p v-else-if="isError" class="py-16 text-center text-sm text-destructive">
      ページの読み込みに失敗しました
    </p>

    <p v-else-if="isNotFound" class="py-16 text-center text-sm text-muted-foreground">
      プロジェクトが見つかりません
    </p>

    <!--
      `:key` でプロジェクトごとに作り直す。

      vike-vue はクライアント遷移で同じ `+Page.vue` に解決される URL 間では
      コンポーネントを差し替えず patch するため、サイドバーからプロジェクトを
      切り替えても設定画面のインスタンスが生き残る。各セクションは setup 時の
      props でクエリを組むものがあり、そのままだと一覧が前のプロジェクトの
      ままで、操作だけが今のプロジェクトへ飛ぶ（メンバー節では、表示されて
      いない側のプロジェクトから実際に人が外れる）。
    -->
    <ProjectSettingsView
      v-else-if="tenantId && project"
      :key="project.id"
      :tenant-id="tenantId"
      :tenant-slug="tenantDisplayId"
      :project="project"
    />
  </div>
</template>
