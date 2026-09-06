<script setup lang="ts">
import { computed, watch } from 'vue';
import { usePageContext } from 'vike-vue/usePageContext';

import NavUser from '@/components/header/NavUser.vue';
import TenantSwitcher from '@/components/header/TenantSwitcher.vue';
import { useAuthSession } from '@/composables/useAuthSession';
import { useAuthStore } from '@/stores/auth';
import { useTenantStore, type Tenant } from '@/stores/tenant';

const pageContext = usePageContext();
const authStore = useAuthStore();
const tenantStore = useTenantStore();
const { logout } = useAuthSession();

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
      <NavUser :user="user" :on-logout="logout" />
    </div>
  </header>
</template>
