import { computed, type MaybeRefOrGetter, toValue } from 'vue';
import { useQuery } from '@tanstack/vue-query';

import { fetchClient } from '@/lib/api-vue-query';
import type { TenantUuid } from '@/lib/api-ids';
import type { components } from '@/generated/api';

const LIST_TENANTS_PATH = '/v1/tenants' as const;
type TenantResponse = components['schemas']['TenantListItemResponse'];

/** Route param (display_id) を GET /v1/tenants で UUID に解決する。 */
export function useResolvedTenantId(tenantDisplayId: MaybeRefOrGetter<string>) {
  const displayId = computed(() => String(toValue(tenantDisplayId) ?? ''));

  const tenantsQuery = useQuery({
    queryKey: ['get', LIST_TENANTS_PATH],
    queryFn: async ({ signal }) => {
      const { data, error } = await fetchClient.GET(LIST_TENANTS_PATH, { signal });
      if (error) throw error;
      return (data ?? []) as TenantResponse[];
    },
    enabled: computed(() => !!displayId.value),
    staleTime: 60_000,
  });

  const resolvedTenant = computed<TenantResponse | null>(() => {
    const data = tenantsQuery.data.value;
    if (!data || !displayId.value) return null;
    return data.find((t) => t.display_id === displayId.value) ?? null;
  });

  const tenantId = computed<TenantUuid | null>(() => {
    const id = resolvedTenant.value?.id;
    return id ? (id as TenantUuid) : null;
  });

  /** 解決したテナントのオーナー。代行系の表示判定（レビュー画面など）が使う。 */
  const tenantOwnerId = computed(() => resolvedTenant.value?.owner_id ?? null);

  const isTenantNotFound = computed(
    () =>
      !!displayId.value &&
      tenantsQuery.isSuccess.value &&
      !tenantsQuery.isFetching.value &&
      tenantId.value === null,
  );

  const isResolving = computed(() => tenantsQuery.isLoading.value);

  const isError = computed(() => tenantsQuery.isError.value);
  const error = computed(() => tenantsQuery.error.value);

  return {
    tenantDisplayId: displayId,
    tenantId,
    tenantOwnerId,
    isTenantNotFound,
    isResolving,
    isError,
    error,
    tenantsQuery,
  };
}
