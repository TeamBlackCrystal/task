<script setup lang="ts">
import { PhGear } from '@phosphor-icons/vue';
import { computed, watch } from 'vue';
import { usePageContext } from 'vike-vue/usePageContext';

import NavUser from '@/components/header/NavUser.vue';
import TenantSwitcher from '@/components/header/TenantSwitcher.vue';
import { useAuthSession } from '@/composables/useAuthSession';
import { useMeQuery } from '@/lib/api-vue-query';
import { useAuthStore } from '@/stores/auth';
import { useTenantStore, type Tenant } from '@/stores/tenant';

const pageContext = usePageContext();
const authStore = useAuthStore();
const tenantStore = useTenantStore();
const { logout } = useAuthSession();
const meQuery = useMeQuery();

const tenantSlug = computed(() => {
  const { tenant } = pageContext.routeParams;
  return typeof tenant === 'string' ? tenant : '';
});

watch(tenantSlug, (slug) => void tenantStore.loadTenants(slug || undefined), { immediate: true });

function selectTenant(tenant: Tenant) {
  tenantStore.selectTenant(tenant);
  if (tenant.display_id !== tenantSlug.value) {
    // テナントに紐づく状態を残さないためフルページ遷移にする
    window.location.assign(`/${tenant.display_id}/my-tasks`);
  }
}

/** テナント設定を見ているあいだは歯車に印を付ける。 */
const isTenantSettings = computed(() =>
  pageContext.urlPathname.startsWith(`/${tenantSlug.value}/settings`),
);

const canManageGeneral = computed(() => {
  const userId = meQuery.data.value?.id ?? authStore.user?.id;
  const tenant = tenantStore.selectedTenant;
  return !!tenantSlug.value && !!userId && tenant?.owner_id === userId;
});

const user = computed(() => ({
  name: authStore.user?.username ?? 'User',
  email: authStore.user?.email ?? '',
  avatar: authStore.user?.avatar_url ?? '',
}));
</script>

<template>
  <header
    class="flex h-12 shrink-0 items-center gap-2 border-b bg-sidebar px-3"
    data-slot="app-header"
  >
    <TenantSwitcher
      :tenants="tenantStore.tenants"
      :selected-tenant-id="tenantStore.selectedTenantId"
      :loading="tenantStore.isLoading"
      :error="tenantStore.error"
      @select="selectTenant"
      @retry="tenantStore.loadTenants(tenantSlug)"
    />
    <div class="ml-auto flex items-center gap-1">
      <a
        v-if="canManageGeneral"
        :href="`/${tenantSlug}/settings`"
        aria-label="テナント設定"
        title="テナント設定"
        class="flex size-7.5 items-center justify-center rounded-md text-muted-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
        :class="isTenantSettings ? 'bg-sidebar-accent text-sidebar-accent-foreground' : ''"
      >
        <PhGear class="size-4" />
      </a>
      <span class="mx-1 h-4 w-px bg-border" />
      <NavUser :user="user" :on-logout="logout" />
    </div>
  </header>
</template>
