<script setup lang="ts">
import type { SidebarProps } from '@/components/ui/sidebar';
import { useRouteAlignedTenantId } from '@/composables/useRouteAlignedTenantId';
import { useTenantStore } from '@/stores/tenant';
import { useProjectsQuery } from '@/lib/api-vue-query';
import { usePageContext } from 'vike-vue/usePageContext';
import { navigate } from 'vike/client/router';
import { computed, watch } from 'vue';

import { ListTodo } from '@lucide/vue';
import NavMain from '@/components/sidebar/NavMain.vue';
import NavProjects from '@/components/sidebar/NavProjects.vue';
import {
  closeSidebarForProgrammaticNavigate,
  shouldCloseSidebarOnNavigate,
} from '@/components/sidebar/sidebar-navigation';

import { Sidebar, SidebarContent, SidebarRail, useSidebar } from '@/components/ui/sidebar';

const props = withDefaults(defineProps<SidebarProps>(), {
  collapsible: 'icon',
});

const pageContext = usePageContext();
const tenantStore = useTenantStore();

const tenantSlug = computed(() => {
  const { tenant } = pageContext.routeParams;
  return typeof tenant === 'string' ? tenant : '';
});

const myTasksUrl = computed(() => (tenantSlug.value ? `/${tenantSlug.value}/my-tasks` : '#'));

const routeAlignedTenantId = useRouteAlignedTenantId(
  computed(() => tenantStore.tenants),
  tenantSlug,
);

const projectsQuery = useProjectsQuery(routeAlignedTenantId);

const navProjects = computed(() => projectsQuery.data.value ?? []);

const navProjectsLoading = computed(
  () =>
    projectsQuery.isLoading.value ||
    (Boolean(tenantSlug.value) && routeAlignedTenantId.value === null && tenantStore.isLoading),
);

watch(tenantSlug, (slug) => void tenantStore.loadTenants(slug || undefined), { immediate: true });

function retryProjects() {
  void projectsQuery.refetch();
}

// ---- プロジェクト作成導線（編集・削除は各プロジェクトの設定ページへ集約） ----
const { isMobile, setOpenMobile } = useSidebar();

function onCreateProject() {
  // 作成ボタンはリンクではないので、SidebarContent のイベント委譲では閉じられない
  closeSidebarForProgrammaticNavigate(isMobile.value, setOpenMobile);
  void navigate(`/${tenantSlug.value}/projects/new`);
}

const data = computed(() => ({
  navMain: [
    {
      title: 'My Tasks',
      url: myTasksUrl.value,
      icon: ListTodo,
      isActive: pageContext.urlPathname === `/${pageContext.routeParams.tenant}/my-tasks`,
    },
  ],
}));

/** ナビから遷移したらモバイルのサイドバーを閉じる（判定は sidebar-navigation に切り出し）。 */
function closeOnNavigate(event: MouseEvent) {
  if (shouldCloseSidebarOnNavigate(event, isMobile.value)) setOpenMobile(false);
}
</script>

<template>
  <Sidebar v-bind="props">
    <SidebarContent @click="closeOnNavigate">
      <!-- テナント外のページ（/settings/... など）ではテナント文脈が無く、
           リンク先も一覧も作れないためテナント依存のナビ自体を出さない。 -->
      <NavMain v-if="tenantSlug" :items="data.navMain" />
      <NavProjects
        v-if="tenantSlug"
        :tenant-slug="tenantSlug"
        :projects="navProjects"
        :current-path="pageContext.urlPathname"
        :loading="navProjectsLoading"
        :error="projectsQuery.isError.value"
        @retry="retryProjects"
        @create="onCreateProject"
      />
    </SidebarContent>
    <SidebarRail />
  </Sidebar>
</template>
