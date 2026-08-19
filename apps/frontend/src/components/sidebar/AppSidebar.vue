<script setup lang="ts">
import type { SidebarProps } from '@/components/ui/sidebar';
import { useAuthSession } from '@/composables/useAuthSession';
import { useRouteAlignedTenantId } from '@/composables/useRouteAlignedTenantId';
import { useAuthStore } from '@/stores/auth';
import { useTenantStore, type Tenant } from '@/stores/tenant';
import { useProjectsQuery } from '@/lib/api-vue-query';
import { usePageContext } from 'vike-vue/usePageContext';
import { navigate } from 'vike/client/router';
import { computed, watch } from 'vue';

import { ListTodo } from '@lucide/vue';
import NavMain from '@/components/sidebar/NavMain.vue';
import NavProjects from '@/components/sidebar/NavProjects.vue';
import NavUser from '@/components/sidebar/NavUser.vue';
import TenantSwitcher from '@/components/sidebar/TenantSwitcher.vue';

import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarHeader,
  SidebarRail,
} from '@/components/ui/sidebar';

const props = withDefaults(defineProps<SidebarProps>(), {
  collapsible: 'icon',
});

const pageContext = usePageContext();
const authStore = useAuthStore();
const tenantStore = useTenantStore();
const { logout } = useAuthSession();

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

function selectTenant(tenant: Tenant) {
  tenantStore.selectTenant(tenant);
  if (tenant.display_id !== tenantSlug.value) {
    // Use a full navigation so tenant-scoped application state is reset.
    window.location.assign(`/${tenant.display_id}/my-tasks`);
  }
}

function retryProjects() {
  void projectsQuery.refetch();
}

// ---- プロジェクト作成導線（編集・削除は各プロジェクトの設定ページへ集約） ----
function onCreateProject() {
  void navigate(`/${tenantSlug.value}/projects/new`);
}

const data = computed(() => ({
  user: {
    name: authStore.user?.username ?? 'User',
    email: authStore.user?.email ?? '',
    avatar: authStore.user?.avatar_url ?? '',
  },
  navMain: [
    {
      title: 'My Tasks',
      url: myTasksUrl.value,
      icon: ListTodo,
      isActive: pageContext.urlPathname === `/${pageContext.routeParams.tenant}/my-tasks`,
    },
  ],
}));
</script>

<template>
  <Sidebar v-bind="props">
    <SidebarHeader>
      <TenantSwitcher
        :tenants="tenantStore.tenants"
        :selected-tenant-id="tenantStore.selectedTenantId"
        :loading="tenantStore.isLoading"
        :error="tenantStore.error"
        @select="selectTenant"
        @retry="tenantStore.loadTenants(tenantSlug)"
      />
    </SidebarHeader>
    <SidebarContent>
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
    <SidebarFooter>
      <NavUser :user="data.user" :on-logout="logout" />
    </SidebarFooter>
    <SidebarRail />
  </Sidebar>
</template>
