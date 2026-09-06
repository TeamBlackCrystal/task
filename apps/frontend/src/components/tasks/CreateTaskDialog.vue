<script setup lang="ts">
import { Loader2, X } from '@lucide/vue';
import { computed, ref, watch } from 'vue';
import { useQueryClient } from '@tanstack/vue-query';
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';

import HydrationSafeForm from '@/components/HydrationSafeForm.vue';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import MarkdownEditor from '@/components/markdown/MarkdownEditor.vue';
import type { components } from '@/generated/api';
import { apiClient } from '@/lib/api-vue-query';
import { PRIORITY_CONFIG } from '@/lib/task-display';
import { toIsoDate } from '@/lib/task-date';

const CREATE_TASK_PATH = '/v1/tenants/{tenant_id}/projects/{project_id}/tasks' as const;

type Priority = components['schemas']['TaskPriority'];
type Status = components['schemas']['ProjectStatusResponse'];
type LabelOption = components['schemas']['LabelResponse'];
type CreatedTask = components['schemas']['TaskDetailResponse'];

const priorityOptions = Object.entries(PRIORITY_CONFIG) as [
  Priority,
  (typeof PRIORITY_CONFIG)[Priority],
][];

const props = defineProps<{
  open: boolean;
  tenantId: string;
  projectId: string;
  projectKey: string;
  statuses: Status[];
  /** undefined は未取得（ロード中・エラー）。正常な 0 件は空配列で渡すこと */
  labels?: LabelOption[];
  labelsLoading?: boolean;
  /** ラベル一覧が手元に無いときだけ true にすること（使えるキャッシュがあれば false） */
  labelsError?: boolean;
}>();

const emit = defineEmits<{
  'update:open': [value: boolean];
  created: [task: CreatedTask];
  retryLabels: [];
}>();

const queryClient = useQueryClient();
const title = ref('');
const statusId = ref('');
const description = ref('');
const softDeadline = ref('');
const hardDeadline = ref('');
const priority = ref<Priority>('Medium');
const selectedLabelIds = ref<string[]>([]);
const validationMessage = ref<string | null>(null);
const requestError = ref<string | null>(null);
const successMessage = ref<string | null>(null);

const defaultStatusId = computed(
  () => props.statuses.find((status) => status.is_default)?.id ?? props.statuses[0]?.id ?? '',
);

const createMutation = apiClient.useMutation('post', CREATE_TASK_PATH);

watch(
  () => [props.open, defaultStatusId.value] as const,
  ([open]) => {
    if (open && !props.statuses.some((status) => status.id === statusId.value)) {
      statusId.value = defaultStatusId.value;
    }
  },
  { immediate: true },
);

// プロジェクトが切り替わったら旧プロジェクトの入力（特にラベル ID）を持ち越さない
watch(
  () => props.projectId,
  () => resetForm(),
);

// ラベル一覧の正常取得後、削除済み ID を選択から落とす。
// undefined はロード中・エラー（一覧が不明）なので選択を保持する
watch(
  () => props.labels,
  (labels) => {
    if (!labels) return;
    const ids = new Set(labels.map((label) => label.id));
    selectedLabelIds.value = selectedLabelIds.value.filter((id) => ids.has(id));
  },
);

// 閉じたら入力と結果表示を捨てる。
//
// onOpenChange 側だけで捨てていたときは、作成に成功して親が open を false にする
// 経路（created を受けてダイアログを閉じる）がここを通らないため、「タスクを
// 作成しました」が残り、次に開いた瞬間に前回の成功が表示されていた。
watch(
  () => props.open,
  (open) => {
    if (!open) resetForm();
  },
);

function onOpenChange(value: boolean) {
  if (!value && createMutation.isPending.value) return;
  // 親が open を握っていない場合でも捨てられるよう、ここでも呼ぶ。
  // resetForm は冪等なので watch と二重に走っても問題ない
  if (!value) resetForm();
  emit('update:open', value);
}

function toggleLabel(labelId: string) {
  selectedLabelIds.value = selectedLabelIds.value.includes(labelId)
    ? selectedLabelIds.value.filter((id) => id !== labelId)
    : [...selectedLabelIds.value, labelId];
}

function resetForm() {
  title.value = '';
  statusId.value = defaultStatusId.value;
  description.value = '';
  softDeadline.value = '';
  hardDeadline.value = '';
  priority.value = 'Medium';
  selectedLabelIds.value = [];
  validationMessage.value = null;
  requestError.value = null;
  successMessage.value = null;
}

async function submit() {
  if (createMutation.isPending.value) return;

  const normalizedTitle = title.value.trim();
  validationMessage.value = null;
  requestError.value = null;
  successMessage.value = null;

  if (!normalizedTitle) {
    validationMessage.value = 'タイトルを入力してください';
    return;
  }
  if (!statusId.value) {
    validationMessage.value = 'ステータスを選択してください';
    return;
  }

  const body: components['schemas']['CreateTaskRequest'] = {
    title: normalizedTitle,
    status_id: statusId.value,
    priority: priority.value,
  };
  const normalizedDescription = description.value.trim();
  if (normalizedDescription) body.description = normalizedDescription;
  if (softDeadline.value) body.soft_deadline = toIsoDate(softDeadline.value);
  if (hardDeadline.value) body.hard_deadline = toIsoDate(hardDeadline.value);
  if (selectedLabelIds.value.length) body.label_ids = selectedLabelIds.value;

  try {
    const created = await createMutation.mutateAsync({
      params: { path: { tenant_id: props.tenantId, project_id: props.projectId } },
      body,
    });
    void queryClient.invalidateQueries({
      queryKey: ['get', CREATE_TASK_PATH],
      refetchType: 'none',
    });
    emit('created', created);
    resetForm();
    successMessage.value = 'タスクを作成しました';
  } catch {
    requestError.value = 'タスクの作成に失敗しました。もう一度お試しください';
  }
}
</script>

<template>
  <Dialog v-if="open" :open="true" @update:open="onOpenChange">
    <DialogContent class="max-h-[90vh] overflow-y-auto" :show-close-button="false">
      <DialogHeader class="relative mb-1 pr-10">
        <DialogTitle>新規タスク</DialogTitle>
        <DialogDescription> {{ projectKey }} にタスクを追加します </DialogDescription>
        <div class="absolute -top-2 -right-2">
          <DialogClose as-child>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              aria-label="閉じる"
              :disabled="createMutation.isPending.value"
            >
              <X class="size-4" />
            </Button>
          </DialogClose>
        </div>
      </DialogHeader>

      <HydrationSafeForm v-slot="{ isHydrated }" class="space-y-4" @submit="submit">
        <div class="space-y-1.5">
          <Label for="task-title"> タイトル <span class="text-destructive">*</span> </Label>
          <Input id="task-title" v-model="title" name="title" autocomplete="off" autofocus />
        </div>

        <div class="space-y-1.5">
          <Label for="task-status"> ステータス <span class="text-destructive">*</span> </Label>
          <Select v-model="statusId">
            <SelectTrigger id="task-status" class="w-full">
              <SelectValue placeholder="選択してください" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem v-for="status in statuses" :key="status.id" :value="status.id">
                {{ status.name }}
              </SelectItem>
            </SelectContent>
          </Select>
          <input type="hidden" name="status_id" :value="statusId" />
        </div>

        <div class="space-y-1.5">
          <!-- CodeMirror の実体は contenteditable で label の for が効かないため、
               名前は editor 側の aria-label で与える -->
          <Label>説明</Label>
          <MarkdownEditor
            v-model="description"
            aria-label="説明"
            placeholder="markdown で書けます"
            min-height-class="min-h-24"
          />
        </div>

        <div class="grid gap-4 sm:grid-cols-2">
          <div class="space-y-1.5">
            <Label for="task-soft-deadline">期限</Label>
            <Input
              id="task-soft-deadline"
              v-model="softDeadline"
              name="soft_deadline"
              type="date"
            />
          </div>
          <div class="space-y-1.5">
            <Label for="task-hard-deadline">最終期限</Label>
            <Input
              id="task-hard-deadline"
              v-model="hardDeadline"
              name="hard_deadline"
              type="date"
            />
          </div>
        </div>

        <div class="space-y-1.5">
          <Label for="task-priority">優先度</Label>
          <Select v-model="priority">
            <SelectTrigger id="task-priority" class="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem v-for="[value, config] in priorityOptions" :key="value" :value="value">
                {{ config.label }}
              </SelectItem>
            </SelectContent>
          </Select>
          <input type="hidden" name="priority" :value="priority" />
        </div>

        <div
          v-if="labelsLoading || labelsError || labels?.length"
          class="space-y-1.5"
          role="group"
          aria-labelledby="task-labels-label"
        >
          <Label id="task-labels-label">ラベル</Label>
          <p v-if="labelsLoading" class="text-xs text-muted-foreground">ラベルを読み込み中...</p>
          <div v-else-if="labelsError" class="flex items-center gap-2">
            <p role="alert" class="text-xs text-destructive">ラベルの取得に失敗しました</p>
            <Button type="button" variant="outline" size="sm" @click="emit('retryLabels')">
              再試行
            </Button>
          </div>
          <div v-else class="flex flex-wrap gap-1.5">
            <button
              v-for="label in labels"
              :key="label.id"
              type="button"
              class="inline-flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-xs font-medium transition-colors"
              :aria-pressed="selectedLabelIds.includes(label.id)"
              :style="
                selectedLabelIds.includes(label.id)
                  ? {
                      backgroundColor: label.color + '1a',
                      borderColor: label.color,
                      color: label.color,
                    }
                  : {}
              "
              :class="
                selectedLabelIds.includes(label.id) ? '' : 'text-muted-foreground hover:bg-muted/40'
              "
              @click="toggleLabel(label.id)"
            >
              <span
                class="inline-block size-2 shrink-0 rounded-full"
                :style="{ backgroundColor: label.color }"
                aria-hidden="true"
              />
              {{ label.name }}
            </button>
          </div>
        </div>

        <p v-if="validationMessage" role="alert" class="text-sm text-destructive">
          {{ validationMessage }}
        </p>
        <p v-if="requestError" role="alert" class="text-sm text-destructive">
          {{ requestError }}
        </p>
        <p v-if="successMessage" role="status" class="text-sm text-emerald-600">
          {{ successMessage }}
        </p>

        <DialogFooter>
          <DialogClose as-child>
            <Button type="button" variant="outline" :disabled="createMutation.isPending.value">
              キャンセル
            </Button>
          </DialogClose>
          <Button
            type="submit"
            :disabled="createMutation.isPending.value || !isHydrated || !statusId"
          >
            <Loader2 v-if="createMutation.isPending.value" class="mr-2 size-4 animate-spin" />
            {{ createMutation.isPending.value ? '作成中...' : '作成' }}
          </Button>
        </DialogFooter>
      </HydrationSafeForm>
    </DialogContent>
  </Dialog>
</template>
