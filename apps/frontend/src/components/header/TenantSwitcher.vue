<script setup lang="ts">
import {
  PhBuildings,
  PhCaretDown,
  PhCheck,
  PhGear,
  PhPlus,
  PhUsers,
  PhWarningCircle,
} from '@phosphor-icons/vue';
import { useQuery } from '@tanstack/vue-query';
import { computed, ref } from 'vue';

import CreateTenantDialog from './CreateTenantDialog.vue';
import type { Tenant } from '@/stores/tenant';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { apiClient, useMeQuery } from '@/lib/api-vue-query';

const MEMBERS_PATH = '/v1/tenants/{tenant_id}/members' as const;

const props = defineProps<{
  tenants: Tenant[];
  selectedTenantId: string | null;
  loading?: boolean;
  error?: string | null;
}>();

const emit = defineEmits<{
  select: [tenant: Tenant];
  retry: [];
}>();

const createDialogOpen = ref(false);
const activeTenant = computed(
  () => props.tenants.find((tenant) => tenant.id === props.selectedTenantId) ?? null,
);
const tenantNotFound = computed(
  () => !props.loading && !props.error && props.tenants.length > 0 && !activeTenant.value,
);

/** 引き金に出す名前。読み込み中・見つからないときはその事情を出す。 */
const triggerLabel = computed(() => {
  if (props.loading) return 'テナントを読み込み中…';
  if (tenantNotFound.value) return '指定されたテナントが見つかりません';
  return activeTenant.value?.name ?? '所属テナントなし';
});

/**
 * 頭のメンバー数。テナントが決まるまでは呼ばない。
 *
 * 参照デザインはここにプラン名も並べるが、テナントにプランの概念が無いので数だけ出す。
 */
const membersQuery = useQuery(
  computed(() => ({
    ...apiClient.queryOptions('get', MEMBERS_PATH, {
      params: { path: { tenant_id: activeTenant.value?.id ?? '' } },
    }),
    enabled: !!activeTenant.value,
    staleTime: 60_000,
    retry: false,
  })),
);

const meQuery = useMeQuery();
const canManageGeneral = computed(
  () => !!activeTenant.value && meQuery.data.value?.id === activeTenant.value.owner_id,
);

const memberSummary = computed(() => {
  const count = membersQuery.data.value?.length;
  return count === undefined ? '' : `${count} 人のメンバー`;
});

/** テナント設定への行き先。テナントが決まっていないと組み立てられない。 */
const settingsHref = computed(() =>
  activeTenant.value ? `/${activeTenant.value.display_id}/settings` : '',
);
const membersHref = computed(() =>
  activeTenant.value ? `/${activeTenant.value.display_id}/settings/members` : '',
);
</script>

<template>
  <DropdownMenu>
    <DropdownMenuTrigger as-child>
      <button
        type="button"
        :disabled="loading"
        class="flex max-w-65 items-center gap-2 rounded-lg border bg-background py-1.5 pr-2 pl-1.5 text-left text-sm shadow-xs hover:bg-accent disabled:opacity-50 data-[state=open]:bg-accent"
      >
        <span
          class="flex aspect-square size-6 shrink-0 items-center justify-center overflow-hidden rounded-md border bg-secondary"
        >
          <img
            v-if="activeTenant?.icon_url"
            :src="activeTenant.icon_url"
            alt=""
            class="size-full object-cover"
          />
          <PhBuildings v-else class="size-3.5 text-muted-foreground" />
        </span>
        <span class="truncate font-medium">{{ triggerLabel }}</span>
        <PhCaretDown class="size-3 shrink-0 text-muted-foreground" />
      </button>
    </DropdownMenuTrigger>

    <DropdownMenuContent class="w-68 rounded-lg p-1" align="start" side="bottom" :side-offset="4">
      <!-- 今どのテナントに居るか。切り替えの前に、開いた人の現在地を出す -->
      <div v-if="activeTenant" class="flex items-center gap-2.5 p-2">
        <span
          class="flex size-8 shrink-0 items-center justify-center overflow-hidden rounded-lg border bg-secondary"
        >
          <img
            v-if="activeTenant.icon_url"
            :src="activeTenant.icon_url"
            alt=""
            class="size-full object-cover"
          />
          <PhBuildings v-else class="size-4 text-muted-foreground" />
        </span>
        <span class="min-w-0 flex-1 leading-snug">
          <span class="block truncate text-sm font-medium">{{ activeTenant.name }}</span>
          <span v-if="memberSummary" class="block text-xs text-muted-foreground">
            {{ memberSummary }}
          </span>
        </span>
      </div>

      <template v-if="activeTenant">
        <DropdownMenuSeparator />
        <DropdownMenuItem v-if="canManageGeneral" as-child class="gap-2">
          <a :href="settingsHref">
            <PhGear class="size-4 shrink-0 text-muted-foreground" />
            <span class="min-w-0 flex-1">テナント設定</span>
          </a>
        </DropdownMenuItem>
        <DropdownMenuItem as-child class="gap-2">
          <a :href="membersHref">
            <PhUsers class="size-4 shrink-0 text-muted-foreground" />
            <span class="min-w-0 flex-1">メンバー</span>
          </a>
        </DropdownMenuItem>
      </template>

      <DropdownMenuSeparator />
      <DropdownMenuLabel class="text-xs font-medium text-muted-foreground">
        テナントを切り替え
      </DropdownMenuLabel>

      <DropdownMenuItem v-if="error" class="gap-2 p-2 text-destructive" @click="emit('retry')">
        <PhWarningCircle class="size-4" />
        {{ error }}（再試行）
      </DropdownMenuItem>
      <DropdownMenuItem v-else-if="!loading && tenants.length === 0" disabled class="p-2">
        所属テナントがありません
      </DropdownMenuItem>
      <DropdownMenuItem
        v-for="tenant in tenants"
        :key="tenant.id"
        class="gap-2"
        @click="emit('select', tenant)"
      >
        <span
          class="flex size-5.5 shrink-0 items-center justify-center overflow-hidden rounded-md border bg-secondary"
        >
          <img
            v-if="tenant.icon_url"
            :src="tenant.icon_url"
            alt=""
            class="size-full object-cover"
          />
          <PhBuildings v-else class="size-3 text-muted-foreground" />
        </span>
        <span class="min-w-0 flex-1 truncate">{{ tenant.name }}</span>
        <!-- 今いるテナントに印を付ける。場所は固定して、行ごとにずれないようにする -->
        <PhCheck
          class="size-3.5 shrink-0"
          :class="tenant.id === selectedTenantId ? 'text-foreground' : 'text-transparent'"
        />
      </DropdownMenuItem>

      <div class="p-1 pt-1.5">
        <Button size="sm" class="w-full gap-1.5" @click="createDialogOpen = true">
          <PhPlus class="size-3.5" />
          テナントを作成
        </Button>
      </div>
    </DropdownMenuContent>
  </DropdownMenu>
  <CreateTenantDialog v-model:open="createDialogOpen" />
</template>
