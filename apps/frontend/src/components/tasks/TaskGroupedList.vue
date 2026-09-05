<script setup lang="ts">
import { computed, nextTick, ref } from 'vue';
import { PhCaretDown, PhPlus } from '@phosphor-icons/vue';
import { CornerDownLeft } from '@lucide/vue';
import { PhCalendarPlus, PhFlag, PhTag } from '@phosphor-icons/vue';
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';

import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import TaskGroupedRow from '@/components/tasks/TaskGroupedRow.vue';
import type { components } from '@/generated/api';
import type { CreateTaskInput, TaskRowField } from '@/composables/useTaskRowMutations';
import type { TaskGroup } from '@/components/tasks/task-grouped-columns';
import {
  localDateInputToIso,
  PRIORITY_CONFIG,
  type ApiPriority as TaskPriority,
} from '@/lib/task-display';
import TaskAssigneePicker from '@/components/tasks/TaskAssigneePicker.vue';
import { TASK_ROW_GRID } from '@/components/tasks/task-grouped-columns';

type TaskResponse = components['schemas']['TaskResponse'];
type LabelResponse = components['schemas']['LabelResponse'];
type StatusResponse = components['schemas']['ProjectStatusResponse'];
type ProjectMember = { id: string; username: string; avatar_url?: string | null };

const props = defineProps<{
  groups: TaskGroup[];
  projectLabels: LabelResponse[];
  members: ProjectMember[];
  /** 担当者候補の取得状態。取得中・失敗を「候補 0 人」と混ぜない */
  membersState?: { loading?: boolean; error?: boolean; onRetry?: () => void };
  statuses: StatusResponse[];
  pending: Record<string, TaskRowField | undefined>;
  errors: Record<string, string | undefined>;
  commentPendingTaskIds?: Record<string, boolean>;
  /** 追加中のグループ。二重送信を止める */
  creatingStatusIds?: Record<string, boolean>;
  /** グループごとの作成失敗。追加行の下に出す */
  createErrors?: Record<string, string | undefined>;
  /** 行からのコメント追加。成功したときだけ下書きを捨てるので成否を返してもらう */
  onComment: (task: TaskResponse, body: string) => Promise<boolean>;
  /** タスクの作成。同上 */
  onCreate: (input: CreateTaskInput) => Promise<boolean>;
}>();

const emit = defineEmits<{
  open: [taskId: string];
  more: [statusId: string];
  'update:status': [task: TaskResponse, statusId: string];
  'update:priority': [task: TaskResponse, priority: TaskResponse['priority']];
  'update:softDeadline': [task: TaskResponse, iso: string | null];
  'toggle:assignee': [task: TaskResponse, userId: string, checked: boolean];
  'toggle:label': [task: TaskResponse, labelId: string, checked: boolean];
}>();

// 折りたたみは画面内の一時状態。URL には載せない（共有したい情報ではない）
const collapsed = ref<Record<string, boolean>>({});

function toggle(statusId: string) {
  collapsed.value = { ...collapsed.value, [statusId]: !collapsed.value[statusId] };
}

// ---- グループ末尾からの追加 ----
// タイトルだけで作る。他の項目は作った行からそのまま触れる
const addingStatusId = ref<string | null>(null);
const draftTitle = ref('');
type InputRef = InstanceType<typeof Input>;
/**
 * v-for の内側に置いた ref は、対象が 1 つでも Vue が配列で入れる
 * （コンパイラが `ref_for` を付けるため）。追加行は同時に 1 つしか開かないので
 * 先頭を取る。素の `.$el` を読むと常に undefined でフォーカスが当たらない。
 */
function focusDraft(target: InputRef | InputRef[] | null) {
  const instance = Array.isArray(target) ? target[0] : target;
  (instance?.$el as HTMLInputElement | undefined)?.focus();
}

const draftInputRef = ref<InputRef | InputRef[] | null>(null);
/** 作成時にその場で決める項目。行と同じピッカーを使う */
const draftAssigneeIds = ref<string[]>([]);
const draftDeadline = ref('');
const draftPriority = ref<TaskPriority | null>(null);
const draftLabelIds = ref<string[]>([]);

const draftAssignees = computed(() =>
  props.members.filter((member) => draftAssigneeIds.value.includes(member.id)),
);
const draftLabels = computed(() =>
  props.projectLabels.filter((label) => draftLabelIds.value.includes(label.id)),
);

function toggleDraftAssignee(userId: string, checked: boolean) {
  draftAssigneeIds.value = checked
    ? [...draftAssigneeIds.value, userId]
    : draftAssigneeIds.value.filter((id) => id !== userId);
}

function toggleDraftLabel(labelId: string, checked: boolean) {
  draftLabelIds.value = checked
    ? [...draftLabelIds.value, labelId]
    : draftLabelIds.value.filter((id) => id !== labelId);
}

const showDraftDeadline = ref(false);
const draftDeadlineRef = ref<InputRef | InputRef[] | null>(null);

async function openDraftDeadline() {
  showDraftDeadline.value = true;
  await nextTick();
  focusDraft(draftDeadlineRef.value);
}

/**
 * 下書きの世代。開き直し・キャンセルのたびに進める。
 *
 * 下書きは全グループで 1 組の ref を共有しているので、作成の await のあいだに
 * キャンセルされたり別のグループ（あるいは同じグループ）で書き始められたら、
 * 手元の下書きはもうその作成のものではない。世代が変わっていたら触らない。
 */
let draftGeneration = 0;

function resetDraft() {
  draftTitle.value = '';
  draftAssigneeIds.value = [];
  draftDeadline.value = '';
  draftPriority.value = null;
  draftLabelIds.value = [];
  showDraftDeadline.value = false;
}

async function startAdding(statusId: string) {
  draftGeneration += 1;
  addingStatusId.value = statusId;
  resetDraft();
  await nextTick();
  focusDraft(draftInputRef.value);
}

function cancelAdding() {
  draftGeneration += 1;
  addingStatusId.value = null;
  resetDraft();
}

async function commitAdding(statusId: string) {
  const title = draftTitle.value.trim();
  if (!title) {
    cancelAdding();
    return;
  }
  const generation = draftGeneration;
  const created = await props.onCreate({
    title,
    statusId,
    assigneeIds: draftAssigneeIds.value,
    softDeadline: draftDeadline.value ? localDateInputToIso(draftDeadline.value) : null,
    priority: draftPriority.value,
    labelIds: draftLabelIds.value,
  });
  // 失敗したら入力を残す（消すと、何が失われたのか分からないまま打ち直しになる）
  if (!created) return;
  // 待っているあいだに下書きが別のものへ入れ替わっていたら消さない
  if (draftGeneration !== generation) return;
  // 続けて足せるように入力欄は開いたままにし、中身だけ空にする
  resetDraft();
}
</script>

<template>
  <div class="flex flex-col gap-4">
    <section v-for="group in groups" :key="group.status.id">
      <div class="flex h-8 items-center gap-2">
        <Button
          type="button"
          variant="ghost"
          size="icon"
          class="size-5"
          :aria-label="`${group.status.name} を折りたたむ`"
          :aria-expanded="!collapsed[group.status.id]"
          @click="toggle(group.status.id)"
        >
          <!-- 2 つのアイコンを差し替えると切り替わるだけになるので、1 つを回して見せる -->
          <PhCaretDown
            class="size-3 transition-transform duration-200 ease-out motion-reduce:transition-none"
            :class="collapsed[group.status.id] && '-rotate-90'"
          />
        </Button>
        <span
          class="inline-flex h-5 items-center gap-1.5 rounded-md border px-2 text-[11px] font-semibold uppercase tracking-wide"
          :style="group.status.color ? { borderColor: group.status.color } : undefined"
        >
          <span
            v-if="group.status.color"
            class="inline-block size-2 rounded-full"
            :style="{ backgroundColor: group.status.color }"
            aria-hidden="true"
          />
          {{ group.status.name }}
        </span>
        <span class="text-xs tabular-nums text-muted-foreground">{{ group.total }}</span>
      </div>

      <!--
        開閉のアニメーション。高さを JS で測らずに済むよう grid-rows を 0fr↔1fr で
        遷移させる（中身の高さが変わっても指定を直さなくてよい）。
      -->
      <div
        class="grid transition-[grid-template-rows] duration-200 ease-out motion-reduce:transition-none"
        :class="collapsed[group.status.id] ? 'grid-rows-[0fr]' : 'grid-rows-[1fr]'"
      >
        <div class="overflow-hidden">
          <div class="overflow-x-auto" :inert="collapsed[group.status.id] || undefined">
            <div :class="[TASK_ROW_GRID, 'border-b text-xs text-muted-foreground']">
              <div class="px-2 py-1.5">タスク</div>
              <div class="px-2">担当</div>
              <div class="px-2">期限</div>
              <div class="px-2">優先度</div>
              <div class="px-2"></div>
            </div>

            <TaskGroupedRow
              v-for="task in group.tasks"
              :key="task.id"
              :task="task"
              :statuses="statuses"
              :project-labels="projectLabels"
              :members="members"
              :pending-field="pending[task.id]"
              :error="errors[task.id]"
              :comment-pending="!!commentPendingTaskIds?.[task.id]"
              @open="emit('open', $event)"
              @update:status="(statusId) => emit('update:status', task, statusId)"
              @update:priority="(priority) => emit('update:priority', task, priority)"
              @update:soft-deadline="(iso) => emit('update:softDeadline', task, iso)"
              @toggle:assignee="(userId, checked) => emit('toggle:assignee', task, userId, checked)"
              @toggle:label="(labelId, checked) => emit('toggle:label', task, labelId, checked)"
              :members-state="membersState"
              :on-comment="(body: string) => onComment(task, body)"
            />

            <!-- 失敗したページは取り直せるようにする。導線が無いと、以降のページへ進めない -->
            <div v-if="group.isError" class="flex min-w-[42rem] items-center gap-2 px-3 py-2">
              <p class="text-sm text-destructive">読み込みに失敗しました</p>
              <Button
                type="button"
                variant="outline"
                size="sm"
                class="h-7 text-xs"
                :disabled="group.isLoading"
                @click="group.retry()"
              >
                再試行
              </Button>
            </div>
            <p
              v-else-if="group.isLoading && !group.tasks.length"
              class="min-w-[42rem] px-3 py-2 text-sm text-muted-foreground"
            >
              読み込み中…
            </p>
            <p
              v-else-if="!group.tasks.length"
              class="min-w-[42rem] px-3 py-2 text-sm text-muted-foreground"
            >
              タスクはありません
            </p>

            <div class="min-w-[42rem]">
              <!--
                追加中の行は、できあがるタスクの行と同じ見た目にする（参照）。
                入力欄を枠で囲うと、行の並びから浮いて別の画面のように見える。
              -->
              <div
                v-if="addingStatusId === group.status.id"
                class="flex items-center gap-2 border-b border-border/60 px-2 py-1.5"
              >
                <span
                  class="size-4 shrink-0 rounded-full border-2 border-dashed"
                  :style="group.status.color ? { borderColor: group.status.color } : undefined"
                  aria-hidden="true"
                />
                <Input
                  ref="draftInputRef"
                  v-model="draftTitle"
                  class="h-7 flex-1 border-0 bg-transparent px-0 text-sm shadow-none focus-visible:ring-0"
                  placeholder="タスク名を入力"
                  :aria-label="`${group.status.name} にタスクを追加`"
                  :disabled="!!creatingStatusIds?.[group.status.id]"
                  @keydown.enter.prevent="commitAdding(group.status.id)"
                  @keydown.esc.prevent="cancelAdding"
                />
                <!-- 作成時にその場で決める（後から行で直せるが、入れ直す手間を省く） -->
                <TaskAssigneePicker
                  :members="members"
                  :selected="draftAssignees"
                  :members-state="membersState"
                  @toggle="toggleDraftAssignee"
                />
                <!-- 参照はアイコンだけ。日付の入力欄は押したときに出す -->
                <Input
                  v-if="showDraftDeadline"
                  ref="draftDeadlineRef"
                  v-model="draftDeadline"
                  type="date"
                  class="h-7 w-36 text-xs"
                  aria-label="期限"
                  @blur="showDraftDeadline = false"
                  @keydown.esc.prevent="showDraftDeadline = false"
                />
                <Button
                  v-else
                  type="button"
                  variant="ghost"
                  size="icon"
                  class="size-7"
                  :class="draftDeadline ? 'text-foreground' : 'text-muted-foreground'"
                  aria-label="期限を設定"
                  @click="openDraftDeadline"
                >
                  <PhCalendarPlus class="size-4" aria-hidden="true" />
                </Button>
                <DropdownMenu>
                  <DropdownMenuTrigger as-child>
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      class="size-7 p-0"
                      :style="
                        draftPriority ? { color: PRIORITY_CONFIG[draftPriority].color } : undefined
                      "
                      :aria-label="
                        draftPriority ? `優先度: ${PRIORITY_CONFIG[draftPriority].label}` : '優先度'
                      "
                    >
                      <PhFlag class="size-4" aria-hidden="true" />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="start">
                    <DropdownMenuRadioGroup
                      :model-value="draftPriority ?? ''"
                      @update:model-value="(v) => (draftPriority = String(v) as TaskPriority)"
                    >
                      <DropdownMenuRadioItem
                        v-for="(config, value) in PRIORITY_CONFIG"
                        :key="value"
                        :value="value"
                        :style="{ color: config.color }"
                      >
                        <PhFlag class="size-4" aria-hidden="true" />
                        {{ config.label }}
                      </DropdownMenuRadioItem>
                    </DropdownMenuRadioGroup>
                  </DropdownMenuContent>
                </DropdownMenu>
                <DropdownMenu>
                  <DropdownMenuTrigger as-child>
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      class="size-7 p-0"
                      :class="draftLabels.length ? 'text-foreground' : 'text-muted-foreground'"
                      :aria-label="
                        draftLabels.length ? `ラベル: ${draftLabels.length}件` : 'ラベル'
                      "
                    >
                      <PhTag class="size-4" aria-hidden="true" />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="start">
                    <p
                      v-if="!projectLabels.length"
                      class="px-2 py-1.5 text-sm text-muted-foreground"
                    >
                      ラベルがありません
                    </p>
                    <DropdownMenuCheckboxItem
                      v-for="label in projectLabels"
                      :key="label.id"
                      :model-value="draftLabelIds.includes(label.id)"
                      @select="(event: Event) => event.preventDefault()"
                      @update:model-value="(v) => toggleDraftLabel(label.id, !!v)"
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

                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  class="h-7 px-2 text-xs"
                  @click="cancelAdding"
                >
                  キャンセル
                </Button>
                <Button
                  type="button"
                  size="sm"
                  class="h-7 gap-1 px-2 text-xs"
                  :disabled="!!creatingStatusIds?.[group.status.id] || !draftTitle.trim()"
                  @click="commitAdding(group.status.id)"
                >
                  保存
                  <CornerDownLeft class="size-3" aria-hidden="true" />
                </Button>
              </div>
              <Button
                v-else
                type="button"
                variant="ghost"
                size="sm"
                class="ml-2 h-7 gap-1.5 px-2 text-xs text-muted-foreground"
                @click="startAdding(group.status.id)"
              >
                <PhPlus class="size-3.5" />
                タスクを追加
              </Button>
              <p
                v-if="createErrors?.[group.status.id]"
                class="px-2 pb-1.5 text-xs text-destructive"
              >
                {{ createErrors[group.status.id] }}
              </p>
            </div>

            <div v-if="group.hasMore" class="px-2 py-1">
              <Button
                type="button"
                variant="ghost"
                size="sm"
                class="h-7 text-xs"
                :disabled="group.isLoading"
                @click="emit('more', group.status.id)"
              >
                もっと見る（残り {{ group.total - group.tasks.length }} 件）
              </Button>
            </div>
          </div>
        </div>
      </div>
    </section>
  </div>
</template>
