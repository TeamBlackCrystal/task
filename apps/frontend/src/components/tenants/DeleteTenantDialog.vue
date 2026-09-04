<script setup lang="ts">
import { useQueryClient } from '@tanstack/vue-query';
import { computed, ref, watch } from 'vue';

import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Label } from '@/components/ui/label';
import { apiClient } from '@/lib/api-vue-query';
import type { components } from '@/generated/api';

type TenantResponse = components['schemas']['TenantResponse'];

const LIST_TENANTS_PATH = '/v1/tenants' as const;
const TENANT_PATH = '/v1/tenants/{id}' as const;

const props = defineProps<{
  open: boolean;
  tenant: TenantResponse;
}>();

const emit = defineEmits<{
  'update:open': [open: boolean];
  deleted: [tenant: TenantResponse];
}>();

const queryClient = useQueryClient();
const deleteError = ref<string | null>(null);
const confirmText = ref('');

const deleteMutation = apiClient.useMutation('delete', TENANT_PATH);

/** 打ち間違いで消えないよう、URL と同じ文字列を打たせる。 */
const canDelete = computed(() => confirmText.value === props.tenant.display_id);

watch(
  () => props.open,
  (open) => {
    if (open) {
      deleteError.value = null;
      confirmText.value = '';
    }
  },
);

function onOpenChange(open: boolean) {
  // 削除の最中は閉じない（結果を見逃さないため）
  if (!open && deleteMutation.isPending.value) return;
  emit('update:open', open);
}

async function confirmDelete() {
  if (!canDelete.value) return;
  deleteError.value = null;
  try {
    await deleteMutation.mutateAsync({ params: { path: { id: props.tenant.id } } });
    await queryClient.invalidateQueries({ queryKey: ['get', LIST_TENANTS_PATH] });
    emit('update:open', false);
    emit('deleted', props.tenant);
  } catch {
    deleteError.value = 'テナントを削除できませんでした';
  }
}
</script>

<template>
  <Dialog v-if="open" :open="true" @update:open="onOpenChange">
    <DialogContent class="max-w-[440px]" :show-close-button="false">
      <DialogHeader>
        <DialogTitle>「{{ tenant.name }}」を削除しますか？</DialogTitle>
        <DialogDescription>
          この操作は取り消せません。このテナントのプロジェクト・タスク・ラベル・招待がすべて完全に削除されます。
        </DialogDescription>
      </DialogHeader>

      <div>
        <Label for="delete-tenant-confirm" class="mb-1.5 text-[13px] font-medium">
          確認のため <span class="font-mono">{{ tenant.display_id }}</span> と入力してください
        </Label>
        <input
          id="delete-tenant-confirm"
          v-model="confirmText"
          :placeholder="tenant.display_id"
          class="h-9 w-full rounded-md border bg-background px-3 font-mono text-[13px] shadow-sm outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50"
        />
      </div>

      <p v-if="deleteError" role="alert" class="text-sm text-destructive">{{ deleteError }}</p>

      <DialogFooter>
        <Button variant="outline" @click="onOpenChange(false)">キャンセル</Button>
        <Button
          variant="destructive"
          :disabled="!canDelete || deleteMutation.isPending.value"
          @click="confirmDelete"
        >
          テナントを削除
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
