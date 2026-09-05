<script setup lang="ts">
import { computed, nextTick, ref } from 'vue';
import { PhCalendarBlank, PhCalendarPlus, PhChat, PhFlag, PhTag } from '@phosphor-icons/vue';

import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import {
  formatDeadline,
  isoToLocalDateInput,
  localDateInputToIso,
  PRIORITY_CONFIG,
} from '@/lib/task-display';
import type { components } from '@/generated/api';
import type { TaskRowField } from '@/composables/useTaskRowMutations';
import { TASK_ROW_GRID } from '@/components/tasks/task-grouped-columns';
import TaskAssigneePicker from '@/components/tasks/TaskAssigneePicker.vue';

type TaskResponse = components['schemas']['TaskResponse'];
type LabelResponse = components['schemas']['LabelResponse'];
type StatusResponse = components['schemas']['ProjectStatusResponse'];
type ProjectMember = { id: string; username: string; avatar_url?: string | null };

const props = defineProps<{
  task: TaskResponse;
  statuses: StatusResponse[];
  projectLabels: LabelResponse[];
  members: ProjectMember[];
  /** 担当者候補の取得状態。取得中・失敗を「候補 0 人」と混ぜない */
  membersState?: { loading?: boolean; error?: boolean; onRetry?: () => void };
  /** 更新中の項目。飛行中は同じ行の操作を止める */
  pendingField?: TaskRowField;
  error?: string;
  commentPending?: boolean;
  /** コメントの追加。成功したときだけ下書きを捨てるので、成否を返してもらう */
  onComment: (body: string) => Promise<boolean>;
}>();

const emit = defineEmits<{
  open: [taskId: string];
  'update:status': [statusId: string];
  'update:priority': [priority: TaskResponse['priority']];
  'update:softDeadline': [iso: string | null];
  'toggle:assignee': [userId: string, checked: boolean];
  'toggle:label': [labelId: string, checked: boolean];
}>();

// 表示（並び・ラベル・色・アイコン）はテーブル表示と同じ定義を使う。
// ここで別に持つと同じ優先度が 2 つの見た目で出る
const PRIORITIES = Object.keys(PRIORITY_CONFIG) as TaskResponse['priority'][];

const isBusy = computed(() => props.pendingField !== undefined);
const deadline = computed(() => formatDeadline(props.task.soft_deadline));
const assignees = computed(() => props.task.assignees.map((entry) => entry.user));
const statusColor = computed(
  () => props.statuses.find((status) => status.id === props.task.status_id)?.color ?? null,
);
const statusName = computed(
  () => props.statuses.find((status) => status.id === props.task.status_id)?.name ?? '—',
);

// ---- 期限のインライン編集 ----
const editingDeadline = ref(false);
const deadlineDraft = ref('');
const deadlineInputRef = ref<InstanceType<typeof Input> | null>(null);

async function startEditingDeadline() {
  if (isBusy.value) return;
  deadlineDraft.value = isoToLocalDateInput(props.task.soft_deadline);
  editingDeadline.value = true;
  await nextTick();
  (deadlineInputRef.value?.$el as HTMLInputElement | undefined)?.focus();
}

function commitDeadline() {
  if (!editingDeadline.value) return;
  editingDeadline.value = false;
  const raw = deadlineDraft.value.trim();
  const current = isoToLocalDateInput(props.task.soft_deadline);
  if (raw === current) return;
  // 変換は詳細画面と同じヘルパーを使う（画面ごとに時刻の寄せ方が変わらないように）
  emit('update:softDeadline', raw ? localDateInputToIso(raw) : null);
}

// ---- コメントのその場追加 ----
const commentOpen = ref(false);
const commentDraft = ref('');
const commentInputRef = ref<InstanceType<typeof Textarea> | null>(null);

async function toggleComment() {
  commentOpen.value = !commentOpen.value;
  if (!commentOpen.value) return;
  await nextTick();
  (commentInputRef.value?.$el as HTMLTextAreaElement | undefined)?.focus();
}

async function submitComment() {
  const body = commentDraft.value.trim();
  if (!body || props.commentPending) return;
  // 失敗したら下書きを残したまま欄を開けておく。閉じてから失敗を知らせても
  // 本文が戻らず、投稿できたと誤解させる
  if (!(await props.onComment(body))) return;
  commentDraft.value = '';
  commentOpen.value = false;
}

// ---- ラベル ----
const labelQuery = ref('');
const visibleLabels = computed(() => {
  const query = labelQuery.value.trim().toLowerCase();
  if (!query) return props.projectLabels;
  return props.projectLabels.filter((label) => label.name.toLowerCase().includes(query));
});

function hasLabel(labelId: string) {
  return props.task.labels.some((label) => label.id === labelId);
}
</script>

<template>
  <div class="group border-b border-border/60 last:border-b-0">
    <div :class="[TASK_ROW_GRID, 'transition-colors hover:bg-muted/40']">
      <!-- 名前。タイトルの変更は詳細だけ（ここでは開く導線のみ） -->
      <div class="flex min-w-0 items-center gap-2 px-2 py-1.5">
        <!-- グループがステータスなので列は持たず、名前の左の丸から変える（参照デザイン） -->
        <DropdownMenu>
          <DropdownMenuTrigger as-child>
            <button
              type="button"
              class="grid size-4 shrink-0 place-items-center rounded-full border-2 disabled:opacity-50"
              :style="statusColor ? { borderColor: statusColor } : undefined"
              :aria-label="`ステータス: ${statusName}`"
              :title="statusName"
              :disabled="isBusy"
            >
              <span
                v-if="statusColor"
                class="block size-1.5 rounded-full"
                :style="{ backgroundColor: statusColor }"
                aria-hidden="true"
              />
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start">
            <DropdownMenuRadioGroup
              :model-value="task.status_id"
              @update:model-value="(v) => emit('update:status', String(v))"
            >
              <DropdownMenuRadioItem v-for="status in statuses" :key="status.id" :value="status.id">
                {{ status.name }}
              </DropdownMenuRadioItem>
            </DropdownMenuRadioGroup>
          </DropdownMenuContent>
        </DropdownMenu>

        <button
          type="button"
          class="min-w-0 truncate text-left text-sm hover:underline"
          @click="emit('open', task.id)"
        >
          {{ task.title }}
        </button>

        <span
          v-for="label in task.labels"
          :key="label.id"
          class="inline-flex shrink-0 items-center gap-1 rounded-full border px-2 py-0.5 text-[11px] text-muted-foreground"
        >
          <span
            class="inline-block size-2 rounded-full"
            :style="{ backgroundColor: label.color }"
            aria-hidden="true"
          />
          {{ label.name }}
        </span>

        <!-- ラベルの付け外し。行にカーソルを合わせたときだけ出す -->
        <DropdownMenu>
          <DropdownMenuTrigger as-child>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              class="size-6 shrink-0 opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100 data-[state=open]:opacity-100"
              :disabled="isBusy"
              aria-label="ラベルを編集"
              title="ラベル"
            >
              <PhTag class="size-4" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" class="w-56">
            <div class="p-1">
              <Input
                v-model="labelQuery"
                class="h-8"
                placeholder="ラベルを検索"
                aria-label="ラベルを検索"
              />
            </div>
            <p v-if="!projectLabels.length" class="px-2 py-1.5 text-sm text-muted-foreground">
              ラベルがありません
            </p>
            <p v-else-if="!visibleLabels.length" class="px-2 py-1.5 text-sm text-muted-foreground">
              一致するラベルがありません
            </p>
            <DropdownMenuCheckboxItem
              v-for="label in visibleLabels"
              :key="label.id"
              :model-value="hasLabel(label.id)"
              :disabled="isBusy"
              @select="(event: Event) => event.preventDefault()"
              @update:model-value="(v) => emit('toggle:label', label.id, !!v)"
            >
              <span
                class="inline-block size-2.5 shrink-0 rounded-full"
                :style="{ backgroundColor: label.color }"
                aria-hidden="true"
              />
              {{ label.name }}
            </DropdownMenuCheckboxItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      <!-- 担当者 -->
      <div class="px-2">
        <TaskAssigneePicker
          :members="members"
          :selected="assignees"
          :disabled="isBusy"
          :members-state="membersState"
          @toggle="(userId, checked) => emit('toggle:assignee', userId, checked)"
        />
      </div>

      <!-- 期限 -->
      <div class="px-2 text-sm">
        <Input
          v-if="editingDeadline"
          ref="deadlineInputRef"
          v-model="deadlineDraft"
          type="date"
          class="h-7"
          aria-label="期限"
          @keydown.enter.prevent="commitDeadline"
          @keydown.esc.prevent="editingDeadline = false"
          @blur="commitDeadline"
        />
        <button
          v-else-if="deadline"
          type="button"
          class="inline-flex items-center gap-1.5 rounded px-1 py-0.5 hover:bg-muted"
          :class="deadline.overdue ? 'text-red-500' : 'text-muted-foreground'"
          :disabled="isBusy"
          @click="startEditingDeadline"
        >
          <PhCalendarBlank class="size-4" />
          {{ deadline.label }}
        </button>
        <Button
          v-else
          type="button"
          variant="ghost"
          size="icon"
          class="size-7 text-muted-foreground"
          :disabled="isBusy"
          aria-label="期限を設定"
          @click="startEditingDeadline"
        >
          <PhCalendarPlus class="size-4" />
        </Button>
      </div>

      <!-- 優先度 -->
      <div class="px-2">
        <DropdownMenu>
          <DropdownMenuTrigger as-child>
            <!-- アイコンと文字の両方に優先度の色を載せる（モックと同じ） -->
            <Button
              type="button"
              variant="ghost"
              size="sm"
              class="h-7 gap-1.5 px-2 text-sm font-normal"
              :style="{ color: PRIORITY_CONFIG[task.priority].color }"
              :disabled="isBusy"
              aria-label="優先度"
            >
              <PhFlag class="size-4" aria-hidden="true" />
              {{ PRIORITY_CONFIG[task.priority].label }}
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start">
            <DropdownMenuRadioGroup
              :model-value="task.priority"
              @update:model-value="
                (v) => emit('update:priority', String(v) as TaskResponse['priority'])
              "
            >
              <DropdownMenuRadioItem
                v-for="value in PRIORITIES"
                :key="value"
                :value="value"
                :style="{ color: PRIORITY_CONFIG[value].color }"
              >
                <PhFlag class="size-4" aria-hidden="true" />
                {{ PRIORITY_CONFIG[value].label }}
              </DropdownMenuRadioItem>
            </DropdownMenuRadioGroup>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      <!-- コメント -->
      <div class="px-2">
        <Button
          type="button"
          variant="ghost"
          size="sm"
          class="h-7 gap-1.5 px-2 text-xs text-muted-foreground"
          aria-label="コメントを追加"
          @click="toggleComment"
        >
          <PhChat class="size-4" />
        </Button>
      </div>
    </div>

    <div v-if="commentOpen" class="flex items-start gap-2 border-t bg-muted/20 px-3 py-2">
      <Textarea
        ref="commentInputRef"
        v-model="commentDraft"
        rows="2"
        class="min-h-0 flex-1 text-sm"
        placeholder="コメントを追加"
        aria-label="コメント"
        :disabled="commentPending"
        @keydown.enter.meta.prevent="submitComment"
        @keydown.enter.ctrl.prevent="submitComment"
        @keydown.esc.prevent="commentOpen = false"
      />
      <div class="flex flex-col gap-1">
        <Button
          type="button"
          size="sm"
          :disabled="commentPending || !commentDraft.trim()"
          @click="submitComment"
        >
          送信
        </Button>
        <Button type="button" size="sm" variant="ghost" @click="commentOpen = false">
          閉じる
        </Button>
      </div>
    </div>

    <p v-if="error" class="px-3 pb-1.5 text-xs text-destructive">{{ error }}</p>
  </div>
</template>
