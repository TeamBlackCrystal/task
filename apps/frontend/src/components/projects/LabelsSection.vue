<script setup lang="ts">
import { useQuery, useQueryClient } from '@tanstack/vue-query';
import { PhPencilSimple, PhPlus, PhTrash } from '@phosphor-icons/vue';
import { computed, ref } from 'vue';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import LabelFormDialog from '@/components/projects/LabelFormDialog.vue';
import { apiClient, projectLabelsQueryOptions } from '@/lib/api-vue-query';
import type { ProjectUuid, TenantUuid } from '@/lib/api-ids';
import type { components } from '@/generated/api';

type LabelResponse = components['schemas']['LabelResponse'];

const LABELS_PATH = '/v1/tenants/{tenant_id}/projects/{project_id}/labels' as const;
const LABEL_PATH = '/v1/tenants/{tenant_id}/projects/{project_id}/labels/{id}' as const;

const props = defineProps<{
  tenantId: string;
  projectId: string;
}>();

const queryClient = useQueryClient();
const isFormOpen = ref(false);
const editingLabel = ref<LabelResponse | null>(null);
const deleteTarget = ref<LabelResponse | null>(null);
const deleteError = ref<string | null>(null);

// 共有ヘルパーに寄せてキー構築・staleTime を一元管理する（#362 の branded 型を渡す。
// props は親で解決済みの UUID）。
// options は computed にして props に追従させる。素の呼び出しだと setup 時の値で
// 固定され、プロジェクトを切り替えたときに一覧だけが前のプロジェクトのまま残る
const labelsQuery = useQuery(
  computed(() =>
    projectLabelsQueryOptions(props.tenantId as TenantUuid, props.projectId as ProjectUuid),
  ),
);
const labels = computed(() => labelsQuery.data.value ?? []);

const deleteMutation = apiClient.useMutation('delete', LABEL_PATH);

function openCreate() {
  editingLabel.value = null;
  isFormOpen.value = true;
}

function openEdit(label: LabelResponse) {
  editingLabel.value = label;
  isFormOpen.value = true;
}

function openDelete(label: LabelResponse) {
  deleteError.value = null;
  deleteTarget.value = label;
}

function onDeleteOpenChange(open: boolean) {
  // 削除リクエスト進行中はダイアログを閉じない（結果の見逃し防止）
  if (!open && deleteMutation.isPending.value) return;
  if (!open) deleteTarget.value = null;
}

async function confirmDelete() {
  const target = deleteTarget.value;
  if (!target) return;
  deleteError.value = null;
  try {
    await deleteMutation.mutateAsync({
      params: {
        path: { tenant_id: props.tenantId, project_id: props.projectId, id: target.id },
      },
    });
    await queryClient.invalidateQueries({ queryKey: ['get', LABELS_PATH] });
    deleteTarget.value = null;
  } catch {
    deleteError.value = 'ラベルを削除できませんでした';
  }
}
</script>

<template>
  <div>
    <div class="mb-5 flex items-center justify-between border-b pb-4">
      <h2 class="text-xl font-semibold">ラベル</h2>
      <Button type="button" variant="outline" @click="openCreate">
        <PhPlus class="size-4" />
        新しいラベル
      </Button>
    </div>

    <p v-if="labelsQuery.isPending.value" class="text-sm text-muted-foreground">読み込み中…</p>

    <p v-else-if="labelsQuery.isError.value" role="alert" class="text-sm text-destructive">
      ラベルを読み込めませんでした
    </p>

    <p v-else-if="labels.length === 0" class="text-sm text-muted-foreground">
      ラベルはまだありません。「新しいラベル」から作成できます。
    </p>

    <ul v-else class="overflow-hidden rounded-lg border">
      <li
        v-for="label in labels"
        :key="label.id"
        class="flex items-center gap-3 border-b px-3.5 py-2.5 last:border-b-0"
      >
        <span
          class="inline-flex shrink-0 items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-xs font-medium"
        >
          <span
            class="size-2 shrink-0 rounded-full"
            :style="{ backgroundColor: label.color }"
            aria-hidden="true"
          />
          {{ label.name }}
        </span>
        <span class="min-w-0 flex-1 truncate text-sm text-muted-foreground">
          {{ label.description }}
        </span>
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          class="size-7 text-muted-foreground"
          :aria-label="`ラベル「${label.name}」を編集`"
          @click="openEdit(label)"
        >
          <PhPencilSimple class="size-4" />
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          class="size-7 text-muted-foreground"
          :aria-label="`ラベル「${label.name}」を削除`"
          @click="openDelete(label)"
        >
          <PhTrash class="size-4" />
        </Button>
      </li>
    </ul>

    <LabelFormDialog
      v-if="isFormOpen"
      :tenant-id="tenantId"
      :project-id="projectId"
      :label="editingLabel"
      @close="isFormOpen = false"
    />

    <Dialog v-if="deleteTarget" :open="true" @update:open="onDeleteOpenChange">
      <DialogContent class="max-w-md" :show-close-button="false">
        <DialogHeader>
          <DialogTitle>ラベルを削除しますか？</DialogTitle>
          <DialogDescription>
            「{{ deleteTarget.name }}」を削除します。この操作は取り消せません。
          </DialogDescription>
        </DialogHeader>
        <p v-if="deleteError" role="alert" class="text-sm text-destructive">{{ deleteError }}</p>
        <DialogFooter>
          <Button
            type="button"
            variant="outline"
            :disabled="deleteMutation.isPending.value"
            @click="deleteTarget = null"
          >
            キャンセル
          </Button>
          <Button
            type="button"
            variant="destructive"
            :disabled="deleteMutation.isPending.value"
            @click="confirmDelete"
          >
            {{ deleteMutation.isPending.value ? '削除中…' : '削除する' }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>
