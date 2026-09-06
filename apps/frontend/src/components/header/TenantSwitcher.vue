<script setup lang="ts">
import { PhBuildings, PhCaretUpDown, PhPlus, PhWarningCircle } from '@phosphor-icons/vue';
import { computed, ref } from 'vue';
import CreateTenantDialog from './CreateTenantDialog.vue';
import type { Tenant } from '@/stores/tenant';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';

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
          class="flex aspect-square size-6 shrink-0 items-center justify-center rounded-md bg-sidebar-primary text-sidebar-primary-foreground"
        >
          <img
            v-if="activeTenant?.icon_url"
            :src="activeTenant.icon_url"
            alt=""
            class="size-4 rounded object-cover"
          />
          <PhBuildings v-else class="size-3.5" />
        </span>
        <span class="truncate font-medium">
          {{
            loading
              ? 'テナントを読み込み中…'
              : tenantNotFound
                ? '指定されたテナントが見つかりません'
                : (activeTenant?.name ?? '所属テナントなし')
          }}
        </span>
        <PhCaretUpDown class="size-4 shrink-0 text-muted-foreground" />
      </button>
    </DropdownMenuTrigger>
    <DropdownMenuContent class="min-w-64 rounded-lg" align="start" side="bottom" :side-offset="4">
      <DropdownMenuLabel class="text-xs text-muted-foreground"> テナント </DropdownMenuLabel>
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
        class="gap-2 p-2"
        @click="emit('select', tenant)"
      >
        <div class="flex size-6 items-center justify-center rounded-sm border">
          <img
            v-if="tenant.icon_url"
            :src="tenant.icon_url"
            alt=""
            class="size-4 rounded object-cover"
          />
          <PhBuildings v-else class="size-3.5 shrink-0" />
        </div>
        {{ tenant.name }}
      </DropdownMenuItem>
      <DropdownMenuSeparator />
      <DropdownMenuItem class="gap-2 p-2" @select="createDialogOpen = true">
        <div class="flex size-6 items-center justify-center rounded-md border bg-transparent">
          <PhPlus class="size-4" />
        </div>
        <div class="font-medium text-muted-foreground">Add tenant</div>
      </DropdownMenuItem>
    </DropdownMenuContent>
  </DropdownMenu>
  <CreateTenantDialog v-model:open="createDialogOpen" />
</template>
