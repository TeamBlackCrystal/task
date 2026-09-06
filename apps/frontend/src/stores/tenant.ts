import { defineStore } from 'pinia';
import { computed, ref } from 'vue';
import type { components } from '@/generated/api';
import { apiClient } from '@/lib/api';

export type Tenant = components['schemas']['TenantListItemResponse'];
export type CreateTenantInput = components['schemas']['CreateTenantRequest'];
export const useTenantStore = defineStore(
  'tenant',
  () => {
    const tenants = ref<Tenant[]>([]);
    const selectedTenantId = ref<string | null>(null);
    const isLoading = ref(false);
    const error = ref<string | null>(null);

    const selectedTenant = computed(
      () => tenants.value.find((tenant) => tenant.id === selectedTenantId.value) ?? null,
    );

    function selectTenant(tenant: Tenant) {
      selectedTenantId.value = tenant.id;
    }

    async function loadTenants(routeTenant?: string) {
      if (isLoading.value) return;

      isLoading.value = true;
      error.value = null;
      try {
        const response = await apiClient.GET('/v1/tenants');
        if (response.error) throw response.error;
        tenants.value = Array.isArray(response.data) ? response.data : [];

        const nextTenant = routeTenant
          ? tenants.value.find(
              (tenant) => tenant.display_id === routeTenant || tenant.id === routeTenant,
            )
          : (tenants.value.find((tenant) => tenant.id === selectedTenantId.value) ??
            tenants.value[0]);
        selectedTenantId.value = nextTenant?.id ?? null;
      } catch (e) {
        console.error('loadTenants failed:', e);
        tenants.value = [];
        selectedTenantId.value = null;
        error.value = 'テナント一覧を取得できませんでした';
      } finally {
        isLoading.value = false;
      }
    }

    async function createTenant(input: CreateTenantInput) {
      const response = await apiClient.POST('/v1/tenants', { body: input });
      if (response.error || !response.data) {
        if (response.response.status === 409) {
          throw new Error('この表示IDはすでに使用されています');
        }
        throw new Error('テナントを作成できませんでした');
      }

      return response.data;
    }

    return {
      tenants,
      selectedTenantId,
      selectedTenant,
      isLoading,
      error,
      selectTenant,
      loadTenants,
      createTenant,
    };
  },
  { persist: { pick: ['selectedTenantId'] } },
);
