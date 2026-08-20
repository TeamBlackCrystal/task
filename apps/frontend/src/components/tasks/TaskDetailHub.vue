<script setup lang="ts">
import { EllipsisVertical, Loader2, Pencil, X } from '@lucide/vue';
import { computed, nextTick, ref } from 'vue';
import type { ComponentPublicInstance } from 'vue';
import type { components } from '@/generated/api';
import AvatarGroup from '@/components/AvatarGroup.vue';
import type { EditableField } from '@/components/tasks/editable-field';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import {
  PRIORITY_CONFIG,
  clampProgressPct,
  formatDeadline,
  formatProgressPct,
  formatTaskDate,
  isoToLocalDateInput,
  taskSeqKey,
} from '@/lib/task-display';
// KFM サイドカー CSS の消費契約 (@/lib/markup-renderer/index.ts): v-html する消費側が
// 明示 import する。GFM は器の kfm-content (KFM_CONTENT_CLASS) が無いと一行も当たらない。
// content-class.ts は leaf module なので、この import で KFM コアが client へ載ることはない。
import { KFM_CONTENT_CLASS } from '@/lib/remark-gfm/content-class';
import '@/lib/remark-gfm/style.css';
import '@/lib/remark-koyori-alerts/style.css';
import '@/lib/rehype-starry-night/style.css';
import '@/lib/remark-kfm-mermaid/style.css';

type TaskDetail = components['schemas']['TaskDetailResponse'];
type StatusOption = components['schemas']['ProjectStatusResponse'];
type LabelOption = components['schemas']['LabelResponse'];

const props = defineProps<{
  task: TaskDetail | null;
  projectKey: string;
  statuses: StatusOption[];
  statusId: string;
  statusUpdating?: boolean;
  statusError?: string | null;
  projectLabels?: LabelOption[];
  projectLabelsLoading?: boolean;
  projectLabelsError?: boolean;
  labelsUpdating?: boolean;
  labelsError?: string | null;
  fieldUpdating?: Partial<Record<EditableField, boolean>>;
  fieldErrors?: Partial<Record<EditableField, string>>;
  loading?: boolean;
  notFound?: boolean;
  error?: boolean;
  deleteDisabled?: boolean;
  /**
   * 'page'（既定）はフルページ用に広画面で 3 カラムへ展開する。
   * 'pane' は分割ビューの狭い右ペイン用に常に 1 カラムで縦積みにする。
   */
  layout?: 'page' | 'pane';
  /**
   * サーバ (+data.ts) の renderDescription 出力。descriptionSource が最新の
   * task.description と厳密一致するときだけ KFM HTML として v-html 表示する。
   * null / 未指定・不一致はプレーンテキスト表示へフォールバックする
   * (分割ビューのペイン等、サーバ生成 HTML を持たない消費側)。
   * v-html に入れてよいのはこの prop だけ — task.description (生テキスト) を
   * v-html へ流す経路を作ってはならない (SSR/sanitize 契約: @/lib/markup-renderer)。
   */
  descriptionHtml?: string | null;
  /**
   * descriptionHtml の描画元テキスト (+data.ts の descriptionSource)。
   * descriptionHtml を渡す消費側は必ず対で渡す。task.description との厳密一致を
   * 下の freshDescriptionHtml が照合し、古い HTML (保存直後の reload 完了前・
   * reload 失敗・他者更新) が v-html に出る経路をコンポーネント側で塞ぐ。
   */
  descriptionSource?: string | null;
}>();

const emit = defineEmits<{
  'update:statusId': [value: string];
  'save:title': [value: string];
  'save:description': [value: string | null];
  'save:progress_pct': [value: number];
  'save:soft_deadline': [value: string | null];
  'save:hard_deadline': [value: string | null];
  'save:label_ids': [value: string[]];
  'delete-request': [];
}>();

const resolvedStatus = computed(() =>
  props.statuses.find((status) => status.id === props.statusId),
);

// 古い HTML の遮断: 描画元 (descriptionSource) がクライアントの最新 task.description と
// 厳密一致しない descriptionHtml は捨て、プレーンテキスト表示へ倒す。v-html の直前で
// 照合するのは、消費側の渡し忘れ・渡し間違いでも stale HTML が漏れないようにするため。
const freshDescriptionHtml = computed(() => {
  if (!props.descriptionHtml || !props.task?.description) return null;
  if (props.descriptionSource !== props.task.description) return null;
  return props.descriptionHtml;
});
const editingField = ref<EditableField | null>(null);
const draftValue = ref('');
const editingControlRef = ref<HTMLElement | ComponentPublicInstance | null>(null);

function isFieldUpdating(field: EditableField) {
  return props.fieldUpdating?.[field] ?? false;
}

function fieldError(field: EditableField) {
  return props.fieldErrors?.[field] ?? null;
}

async function startEditing(field: EditableField) {
  if (!props.task || isFieldUpdating(field)) return;
  editingField.value = field;
  switch (field) {
    case 'title':
      draftValue.value = props.task.title;
      break;
    case 'description':
      draftValue.value = props.task.description ?? '';
      break;
    case 'progress_pct':
      draftValue.value = String(props.task.progress_pct);
      break;
    case 'soft_deadline':
      draftValue.value = isoToLocalDateInput(props.task.soft_deadline);
      break;
    case 'hard_deadline':
      draftValue.value = isoToLocalDateInput(props.task.hard_deadline);
      break;
  }
  await nextTick();
  const control = editingControlRef.value;
  const element =
    control instanceof HTMLElement ? control : (control?.$el as HTMLElement | undefined);
  element?.focus();
}

function cancelEditing() {
  editingField.value = null;
  draftValue.value = '';
}

function commitEditing(field: EditableField) {
  if (!props.task) return;

  switch (field) {
    case 'title': {
      const next = draftValue.value.trim();
      if (!next) {
        cancelEditing();
        return;
      }
      if (next !== props.task.title) emit('save:title', next);
      break;
    }
    case 'description': {
      const trimmed = draftValue.value.trim();
      const current = props.task.description ?? '';
      if (trimmed === current) break;
      emit('save:description', trimmed.length ? trimmed : null);
      break;
    }
    case 'progress_pct': {
      const raw = String(draftValue.value);
      if (!raw.trim()) break;
      const parsed = Number(raw);
      if (!Number.isFinite(parsed)) break;
      const next = clampProgressPct(parsed);
      if (next !== props.task.progress_pct) emit('save:progress_pct', next);
      break;
    }
    case 'soft_deadline': {
      const current = isoToLocalDateInput(props.task.soft_deadline);
      if (draftValue.value === current) break;
      emit('save:soft_deadline', draftValue.value.trim() ? draftValue.value.trim() : null);
      break;
    }
    case 'hard_deadline': {
      const current = isoToLocalDateInput(props.task.hard_deadline);
      if (draftValue.value === current) break;
      emit('save:hard_deadline', draftValue.value.trim() ? draftValue.value.trim() : null);
      break;
    }
  }

  cancelEditing();
}

function onEditKeydown(event: KeyboardEvent, field: EditableField) {
  if (event.key === 'Escape') {
    event.preventDefault();
    cancelEditing();
    return;
  }
  if (event.key === 'Enter' && field !== 'description') {
    event.preventDefault();
    commitEditing(field);
  }
}

function toggleLabel(labelId: string, checked: boolean) {
  // 一覧の取得に失敗している間は、表示しているラベルが現在の集合か判断できない。
  // 引き算には使わない（下記）が、操作の受付自体は止める。UI 側もチェックボックスを
  // disabled にしてあり、押せるのに何も起きない状態にはならない
  if (!props.task || props.labelsUpdating || props.projectLabelsError) return;
  // projectLabels は独立キャッシュで task.labels より古いことがあるため、
  // 交差を取って「一覧に無い = 削除済み」と推定しない（有効なラベルを暗黙解除してしまう）。
  // 実際に削除済みのラベルが混ざって 400 になった場合は保存側でロールバックと再取得を行う
  const current = props.task.labels.map((label) => label.id);
  const next = checked ? [...current, labelId] : current.filter((id) => id !== labelId);
  emit('save:label_ids', next);
}

function clearDeadline(field: 'soft_deadline' | 'hard_deadline') {
  if (isFieldUpdating(field)) return;
  if (field === 'soft_deadline') emit('save:soft_deadline', null);
  else emit('save:hard_deadline', null);
  cancelEditing();
}
</script>

<template>
  <div class="flex flex-col gap-6" data-task-detail-hub>
    <div v-if="loading" class="flex justify-center py-16">
      <Loader2 class="h-8 w-8 animate-spin text-muted-foreground" />
    </div>

    <div
      v-else-if="error"
      class="rounded-lg border border-destructive/30 bg-destructive/5 p-6 text-sm text-destructive"
    >
      タスクの読み込みに失敗しました
    </div>

    <div v-else-if="notFound" class="rounded-lg border p-6 text-sm text-muted-foreground">
      タスクが見つかりません
    </div>

    <template v-else-if="task">
      <header class="flex flex-col gap-2 border-b pb-4">
        <div class="flex flex-wrap items-center gap-2 text-sm text-muted-foreground">
          <slot name="breadcrumb" />
          <span class="font-mono">{{ taskSeqKey(projectKey, task.seq_id) }}</span>
        </div>

        <div v-if="editingField === 'title'" class="flex flex-col gap-1">
          <Input
            v-model="draftValue"
            ref="editingControlRef"
            data-editing="title"
            class="text-2xl font-semibold"
            :disabled="isFieldUpdating('title')"
            aria-label="タイトル"
            @keydown="onEditKeydown($event, 'title')"
            @blur="commitEditing('title')"
          />
          <p v-if="fieldError('title')" class="text-xs text-destructive">
            {{ fieldError('title') }}
          </p>
        </div>
        <button
          v-else
          type="button"
          class="group flex items-start gap-2 text-left"
          :disabled="isFieldUpdating('title')"
          @click="startEditing('title')"
        >
          <h1 class="text-2xl font-semibold tracking-tight group-hover:text-primary">
            {{ task.title }}
          </h1>
          <Pencil
            class="mt-1 size-4 shrink-0 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100"
            aria-hidden="true"
          />
        </button>
        <p v-if="editingField !== 'title' && fieldError('title')" class="text-xs text-destructive">
          {{ fieldError('title') }}
        </p>

        <div class="flex items-center justify-end gap-2">
          <DropdownMenu>
            <DropdownMenuTrigger as-child>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                class="size-8"
                aria-label="タスク操作"
                :disabled="deleteDisabled"
              >
                <EllipsisVertical class="size-4" aria-hidden="true" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem variant="destructive" @select="emit('delete-request')">
                削除
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
          <slot name="header-actions" />
        </div>
      </header>

      <div class="grid grid-cols-1 gap-6" :class="layout === 'pane' ? '' : 'lg:grid-cols-3'">
        <main class="flex flex-col gap-6" :class="layout === 'pane' ? '' : 'lg:col-span-2'">
          <section class="rounded-lg border p-4">
            <div class="mb-2 flex items-center justify-between gap-2">
              <h2 class="text-sm font-medium text-muted-foreground">説明</h2>
              <Button
                v-if="editingField === 'description' && task.description"
                type="button"
                variant="ghost"
                size="sm"
                class="h-7 px-2"
                :disabled="isFieldUpdating('description')"
                @mousedown.prevent
                @click="
                  emit('save:description', null);
                  cancelEditing();
                "
              >
                クリア
              </Button>
              <Button
                v-else-if="editingField !== 'description' && freshDescriptionHtml"
                type="button"
                variant="ghost"
                size="icon"
                class="size-7"
                aria-label="説明を編集"
                :disabled="isFieldUpdating('description')"
                @click="startEditing('description')"
              >
                <Pencil class="size-4" aria-hidden="true" />
              </Button>
            </div>

            <Textarea
              v-if="editingField === 'description'"
              v-model="draftValue"
              ref="editingControlRef"
              data-editing="description"
              class="min-h-28"
              :disabled="isFieldUpdating('description')"
              aria-label="説明"
              @keydown="onEditKeydown($event, 'description')"
              @blur="commitEditing('description')"
            />
            <!--
              KFM 表示は非対話の div にする: 描画 HTML はリンク等の対話要素を含みうるため、
              プレーン表示のような button で包むと入れ子の対話要素になる。編集導線は
              上の鉛筆ボタン。freshDescriptionHtml は task.description との厳密一致を
              照合済みで、説明が空・不一致 (stale) のときは null になり
              下のプレーン分岐へ倒れる。
            -->
            <div
              v-else-if="freshDescriptionHtml"
              :class="KFM_CONTENT_CLASS"
              class="text-sm leading-relaxed"
              data-task-description-html
              v-html="freshDescriptionHtml"
            />
            <button
              v-else
              type="button"
              class="group w-full rounded-md text-left transition-colors hover:bg-muted/40"
              :disabled="isFieldUpdating('description')"
              @click="startEditing('description')"
            >
              <p v-if="task.description" class="whitespace-pre-wrap text-sm leading-relaxed">
                {{ task.description }}
              </p>
              <p v-else class="text-sm text-muted-foreground">
                説明はありません（クリックして追加）
              </p>
            </button>
            <p v-if="fieldError('description')" class="mt-2 text-xs text-destructive">
              {{ fieldError('description') }}
            </p>
          </section>

          <slot name="main" />
        </main>

        <aside class="flex flex-col gap-4">
          <section class="rounded-lg border p-4">
            <h2 class="mb-3 text-sm font-medium text-muted-foreground">ステータス</h2>
            <select
              aria-label="ステータス"
              class="flex h-9 w-full rounded-md border border-input bg-background px-3 text-sm"
              :value="statusId"
              :disabled="statusUpdating"
              @change="emit('update:statusId', ($event.target as HTMLSelectElement).value)"
            >
              <option v-for="status in statuses" :key="status.id" :value="status.id">
                {{ status.name }}
              </option>
            </select>
            <p v-if="statusError" class="mt-2 text-xs text-destructive">{{ statusError }}</p>
            <p
              v-else-if="resolvedStatus"
              class="mt-2 inline-flex items-center rounded-full border px-2 py-0.5 text-xs font-medium"
              :style="{
                backgroundColor: resolvedStatus.color + '1a',
                borderColor: resolvedStatus.color + '66',
                color: resolvedStatus.color,
              }"
            >
              {{ resolvedStatus.name }}
            </p>
          </section>

          <section class="rounded-lg border p-4">
            <h2 class="mb-3 text-sm font-medium text-muted-foreground">進捗</h2>
            <Input
              v-if="editingField === 'progress_pct'"
              v-model="draftValue"
              ref="editingControlRef"
              data-editing="progress_pct"
              type="number"
              min="0"
              max="100"
              :disabled="isFieldUpdating('progress_pct')"
              aria-label="進捗率"
              @keydown="onEditKeydown($event, 'progress_pct')"
              @blur="commitEditing('progress_pct')"
            />
            <button
              v-else
              type="button"
              class="group flex w-full items-center justify-between rounded-md text-left hover:bg-muted/40"
              :disabled="isFieldUpdating('progress_pct')"
              @click="startEditing('progress_pct')"
            >
              <span class="text-sm">{{ formatProgressPct(task.progress_pct) }}</span>
              <Pencil
                class="size-4 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100"
                aria-hidden="true"
              />
            </button>
            <p v-if="fieldError('progress_pct')" class="mt-2 text-xs text-destructive">
              {{ fieldError('progress_pct') }}
            </p>
          </section>

          <section class="rounded-lg border p-4">
            <h2 class="mb-3 text-sm font-medium text-muted-foreground">優先度</h2>
            <div
              class="inline-flex items-center gap-1.5 text-sm"
              :style="{ color: PRIORITY_CONFIG[task.priority].color }"
            >
              <component :is="PRIORITY_CONFIG[task.priority].icon" class="size-4" />
              {{ PRIORITY_CONFIG[task.priority].label }}
            </div>
          </section>

          <section class="rounded-lg border p-4" data-task-labels>
            <div class="mb-3 flex items-center justify-between">
              <h2 class="text-sm font-medium text-muted-foreground">ラベル</h2>
              <DropdownMenu>
                <DropdownMenuTrigger as-child>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    class="size-7"
                    aria-label="ラベルを編集"
                    :disabled="labelsUpdating || projectLabelsLoading"
                  >
                    <Pencil class="size-4" aria-hidden="true" />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end">
                  <p v-if="projectLabelsError" class="px-2 py-1.5 text-sm text-destructive">
                    ラベルを読み込めませんでした
                  </p>
                  <p
                    v-else-if="!projectLabels?.length"
                    class="px-2 py-1.5 text-sm text-muted-foreground"
                  >
                    ラベルがありません
                  </p>
                  <DropdownMenuCheckboxItem
                    v-for="label in projectLabels"
                    :key="label.id"
                    :model-value="task.labels.some((l) => l.id === label.id)"
                    :disabled="labelsUpdating || projectLabelsError"
                    @update:model-value="(v) => toggleLabel(label.id, !!v)"
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
            <div v-if="task.labels.length" class="flex flex-wrap gap-1.5">
              <span
                v-for="label in task.labels"
                :key="label.id"
                class="inline-flex items-center rounded-full border px-2 py-0.5 text-xs font-medium"
                :style="{
                  backgroundColor: label.color + '1a',
                  borderColor: label.color + '66',
                  color: label.color,
                }"
              >
                {{ label.name }}
              </span>
            </div>
            <p v-else class="text-sm text-muted-foreground">ラベルなし</p>
            <p v-if="labelsError" class="mt-2 text-xs text-destructive">{{ labelsError }}</p>
          </section>

          <section class="rounded-lg border p-4">
            <h2 class="mb-3 text-sm font-medium text-muted-foreground">担当者</h2>
            <AvatarGroup
              v-if="task.assignees.length"
              :users="task.assignees.map((a) => a.user)"
              :max-display="5"
              hide-names
            />
            <p v-else class="text-sm text-muted-foreground">未割当</p>
          </section>

          <section class="rounded-lg border p-4">
            <h2 class="mb-3 text-sm font-medium text-muted-foreground">日付</h2>
            <dl class="space-y-3 text-sm">
              <div class="flex flex-col gap-1">
                <dt class="text-muted-foreground">ソフト期限</dt>
                <dd>
                  <div v-if="editingField === 'soft_deadline'" class="flex items-center gap-2">
                    <Input
                      v-model="draftValue"
                      ref="editingControlRef"
                      data-editing="soft_deadline"
                      type="date"
                      class="flex-1"
                      :disabled="isFieldUpdating('soft_deadline')"
                      aria-label="ソフト期限"
                      @keydown="onEditKeydown($event, 'soft_deadline')"
                      @blur="commitEditing('soft_deadline')"
                    />
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      class="size-8 shrink-0"
                      aria-label="ソフト期限をクリア"
                      :disabled="isFieldUpdating('soft_deadline')"
                      @mousedown.prevent
                      @click="clearDeadline('soft_deadline')"
                    >
                      <X class="size-4" />
                    </Button>
                  </div>
                  <button
                    v-else
                    type="button"
                    aria-label="ソフト期限を編集"
                    class="group flex w-full items-center justify-between rounded-md text-left hover:bg-muted/40"
                    :disabled="isFieldUpdating('soft_deadline')"
                    @click="startEditing('soft_deadline')"
                  >
                    <span
                      :class="
                        formatDeadline(task.soft_deadline)?.overdue
                          ? 'text-red-500 font-medium'
                          : ''
                      "
                    >
                      {{
                        formatDeadline(task.soft_deadline)?.label ??
                        (task.soft_deadline
                          ? formatTaskDate(task.soft_deadline)
                          : '未設定（クリックして設定）')
                      }}
                    </span>
                    <Pencil
                      class="size-4 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100"
                      aria-hidden="true"
                    />
                  </button>
                  <p v-if="fieldError('soft_deadline')" class="mt-1 text-xs text-destructive">
                    {{ fieldError('soft_deadline') }}
                  </p>
                </dd>
              </div>

              <div class="flex flex-col gap-1">
                <dt class="text-muted-foreground">ハード期限</dt>
                <dd>
                  <div v-if="editingField === 'hard_deadline'" class="flex items-center gap-2">
                    <Input
                      v-model="draftValue"
                      ref="editingControlRef"
                      data-editing="hard_deadline"
                      type="date"
                      class="flex-1"
                      :disabled="isFieldUpdating('hard_deadline')"
                      aria-label="ハード期限"
                      @keydown="onEditKeydown($event, 'hard_deadline')"
                      @blur="commitEditing('hard_deadline')"
                    />
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      class="size-8 shrink-0"
                      aria-label="ハード期限をクリア"
                      :disabled="isFieldUpdating('hard_deadline')"
                      @mousedown.prevent
                      @click="clearDeadline('hard_deadline')"
                    >
                      <X class="size-4" />
                    </Button>
                  </div>
                  <button
                    v-else
                    type="button"
                    aria-label="ハード期限を編集"
                    class="group flex w-full items-center justify-between rounded-md text-left hover:bg-muted/40"
                    :disabled="isFieldUpdating('hard_deadline')"
                    @click="startEditing('hard_deadline')"
                  >
                    <span
                      :class="
                        formatDeadline(task.hard_deadline)?.overdue
                          ? 'text-red-500 font-medium'
                          : ''
                      "
                    >
                      {{
                        formatDeadline(task.hard_deadline)?.label ??
                        (task.hard_deadline
                          ? formatTaskDate(task.hard_deadline)
                          : '未設定（クリックして設定）')
                      }}
                    </span>
                    <Pencil
                      class="size-4 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100"
                      aria-hidden="true"
                    />
                  </button>
                  <p v-if="fieldError('hard_deadline')" class="mt-1 text-xs text-destructive">
                    {{ fieldError('hard_deadline') }}
                  </p>
                </dd>
              </div>

              <div class="flex justify-between gap-2 border-t pt-2">
                <dt class="text-muted-foreground">作成</dt>
                <dd>{{ formatTaskDate(task.created_at) }}</dd>
              </div>
              <div class="flex justify-between gap-2">
                <dt class="text-muted-foreground">更新</dt>
                <dd>{{ formatTaskDate(task.updated_at) }}</dd>
              </div>
            </dl>
          </section>

          <slot name="sidebar" />
        </aside>
      </div>

      <footer>
        <slot name="footer" />
      </footer>
    </template>
  </div>
</template>
