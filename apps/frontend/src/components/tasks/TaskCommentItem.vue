<script setup lang="ts">
import { Loader2, Pencil, Trash2 } from '@lucide/vue';
import { nextTick, ref } from 'vue';
import type { ComponentPublicInstance } from 'vue';

import { Button } from '@/components/ui/button';
import { Textarea } from '@/components/ui/textarea';
import { formatTaskDate } from '@/lib/task-display';
import type { CommentReply, CommentThread } from '@/composables/useTaskComments';

const props = defineProps<{
  /** スレッド先頭・返信のどちらも同じ形で表示する */
  comment: CommentThread | CommentReply;
  /** このコメントの更新リクエストが進行中 */
  updating?: boolean;
  /** このコメントの削除リクエストが進行中 */
  deleting?: boolean;
  /**
   * 編集の確定。成功で true を返したら編集 UI を閉じる（失敗時は下書きを残す）。
   * 編集・削除ボタンは全コメントに出す — 可否は backend が判定し、
   * 拒否されたら親がエラーメッセージとして拒まれた通りに表示する。
   */
  onUpdate: (body: string) => Promise<boolean>;
  /** 削除の確定。成功で true。 */
  onDelete: () => Promise<boolean>;
}>();

const editing = ref(false);
const editDraft = ref('');
const confirmingDelete = ref(false);
const editControlRef = ref<HTMLElement | ComponentPublicInstance | null>(null);

async function startEditing() {
  if (props.updating || props.deleting) return;
  editDraft.value = props.comment.body ?? '';
  editing.value = true;
  confirmingDelete.value = false;
  await nextTick();
  const control = editControlRef.value;
  const element =
    control instanceof HTMLElement ? control : (control?.$el as HTMLElement | undefined);
  element?.focus();
}

function cancelEditing() {
  editing.value = false;
  editDraft.value = '';
}

async function commitEditing() {
  const next = editDraft.value.trim();
  if (!next || next === (props.comment.body ?? '')) {
    cancelEditing();
    return;
  }
  const saved = await props.onUpdate(next);
  if (saved) cancelEditing();
}

async function confirmDelete() {
  const deleted = await props.onDelete();
  if (deleted) confirmingDelete.value = false;
}
</script>

<template>
  <article class="flex flex-col gap-1" data-task-comment :data-comment-id="comment.id">
    <header class="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-xs text-muted-foreground">
      <span class="font-medium text-foreground">{{ comment.user.name }}</span>
      <span>{{ formatTaskDate(comment.created_at) }}</span>
      <span v-if="comment.updated_at !== comment.created_at">(編集済み)</span>
      <span v-if="updating || deleting" class="inline-flex items-center">
        <Loader2 class="size-3 animate-spin" aria-hidden="true" />
      </span>
      <span v-if="!comment.is_deleted && !editing" class="ml-auto inline-flex items-center gap-1">
        <Button
          type="button"
          variant="ghost"
          size="icon"
          class="size-6"
          aria-label="コメントを編集"
          :disabled="updating || deleting"
          @click="startEditing"
        >
          <Pencil class="size-3.5" aria-hidden="true" />
        </Button>
        <Button
          v-if="!confirmingDelete"
          type="button"
          variant="ghost"
          size="icon"
          class="size-6"
          aria-label="コメントを削除"
          :disabled="updating || deleting"
          @click="confirmingDelete = true"
        >
          <Trash2 class="size-3.5" aria-hidden="true" />
        </Button>
      </span>
    </header>

    <p v-if="comment.is_deleted" class="text-sm text-muted-foreground italic">削除されたコメント</p>

    <div v-else-if="editing" class="flex flex-col gap-2">
      <Textarea
        v-model="editDraft"
        ref="editControlRef"
        class="min-h-20 text-sm"
        :disabled="updating"
        aria-label="コメントを編集"
      />
      <div class="flex justify-end gap-2">
        <Button
          type="button"
          variant="outline"
          size="sm"
          class="h-7 px-2"
          :disabled="updating"
          @click="cancelEditing"
        >
          キャンセル
        </Button>
        <Button
          type="button"
          size="sm"
          class="h-7 px-2"
          :disabled="updating || !editDraft.trim()"
          @click="commitEditing"
        >
          {{ updating ? '保存中…' : '保存' }}
        </Button>
      </div>
    </div>

    <!-- コメント本文は素テキスト表示（whitespace-pre-wrap）。v-html は使わない -->
    <p v-else class="whitespace-pre-wrap text-sm leading-relaxed">{{ comment.body }}</p>

    <div
      v-if="confirmingDelete && !comment.is_deleted && !editing"
      class="flex items-center justify-end gap-2 text-xs"
    >
      <span class="text-muted-foreground">このコメントを削除しますか？</span>
      <Button
        type="button"
        variant="outline"
        size="sm"
        class="h-7 px-2"
        :disabled="deleting"
        @click="confirmingDelete = false"
      >
        キャンセル
      </Button>
      <Button
        type="button"
        variant="destructive"
        size="sm"
        class="h-7 px-2"
        :disabled="deleting"
        @click="confirmDelete"
      >
        {{ deleting ? '削除中…' : '削除する' }}
      </Button>
    </div>
  </article>
</template>
