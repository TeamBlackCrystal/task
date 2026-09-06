<script setup lang="ts">
import { Loader2 } from '@lucide/vue';
import { computed } from 'vue';
import { usePageContext } from 'vike-vue/usePageContext';

import TenantSettingsGeneral from '@/components/tenants/TenantSettingsGeneral.vue';
import { useResolvedTenantId } from '@/composables/useResolvedTenantId';

const pageContext = usePageContext();
const tenantDisplayId = computed(() => String(pageContext.routeParams.tenant ?? ''));

const { resolvedTenant, isTenantNotFound, isResolving, isError } =
  useResolvedTenantId(tenantDisplayId);
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col">
    <div v-if="isResolving" class="flex justify-center py-16">
      <Loader2 class="h-8 w-8 animate-spin text-muted-foreground" />
    </div>

    <p v-else-if="isError" class="py-16 text-center text-sm text-destructive">
      ページの読み込みに失敗しました
    </p>

    <p v-else-if="isTenantNotFound" class="py-16 text-center text-sm text-muted-foreground">
      テナントが見つかりません
    </p>

    <TenantSettingsGeneral
      v-else-if="resolvedTenant"
      :key="resolvedTenant.id"
      :tenant="resolvedTenant"
    />
  </div>
</template>
