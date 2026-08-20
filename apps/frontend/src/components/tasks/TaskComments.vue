<script setup lang="ts">
import { Loader2 } from '@lucide/vue';
import { ref } from 'vue';

import TaskCommentItem from '@/components/tasks/TaskCommentItem.vue';
import { Button } from '@/components/ui/button';
import { Textarea } from '@/components/ui/textarea';
import type { CommentThread } from '@/composables/useTaskComments';

const props = defineProps<{
  threads: CommentThread[];
  /** 一覧の読み込み中 */
  loading?: boolean;
  /** 一覧の読み込み失敗。コメント節の中だけで倒し、ページ本体には影響させない */
  listError?: boolean;
  /** 投稿（新規・返信共通）の進行中 */
  submitPending?: boolean;
  submitError?: string | null;
  /** 更新リクエスト進行中のコメント ID */
  updatingCommentId?: string | null;
  updateError?: string | null;
  /** 削除リクエスト進行中のコメント ID */
  deletingCommentId?: string | null;
  deleteError?: string | null;
  /**
   * 投稿の確定（useTaskComments.submitComment）。成功で true を返したら
   * 下書きを消す。失敗時は下書きを残し、submitError が拒否理由を表示する。
   */
  onSubmit: (body: string, parentCommentId?: string | null) => Promise<boolean>;
  /** 編集の確定（useTaskComments.updateComment） */
  onUpdate: (commentId: string, body: string) => Promise<boolean>;
  /** 削除の確定（useTaskComments.deleteComment） */
  onDelete: (commentId: string) => Promise<boolean>;
}>();

const newDraft = ref('');
/** 返信フォームを開いているスレッドの ID（同時に開くのは一つ） */
const replyTargetId = ref<string | null>(null);
const replyDraft = ref('');

async function submitNewComment() {
  const body = newDraft.value.trim();
  if (!body || props.submitPending) return;
  const posted = await props.onSubmit(body);
  if (posted) newDraft.value = '';
}

function openReply(threadId: string) {
  replyTargetId.value = threadId;
  replyDraft.value = '';
}

function cancelReply() {
  replyTargetId.value = null;
  replyDraft.value = '';
}

async function submitReply(threadId: string) {
  const body = replyDraft.value.trim();
  if (!body || props.submitPending) return;
  const posted = await props.onSubmit(body, threadId);
  if (posted) cancelReply();
}
</script>

<template>
  <section class="rounded-lg border p-4" data-task-comments>
    <h2 class="mb-3 text-sm font-medium text-muted-foreground">コメント</h2>

    <div v-if="loading" class="flex justify-center py-6">
      <Loader2 class="h-5 w-5 animate-spin text-muted-foreground" aria-hidden="true" />
    </div>

    <p v-else-if="listError" class="text-sm text-destructive">コメントを読み込めませんでした</p>

    <template v-else>
      <p v-if="!threads.length" class="text-sm text-muted-foreground">コメントはまだありません</p>

      <ul v-else class="flex flex-col gap-4">
        <li v-for="thread in threads" :key="thread.id" class="flex flex-col gap-2">
          <TaskCommentItem
            :comment="thread"
            :updating="updatingCommentId === thread.id"
            :deleting="deletingCommentId === thread.id"
            :on-update="(body) => onUpdate(thread.id, body)"
            :on-delete="() => onDelete(thread.id)"
          />

          <div v-if="thread.replies.length" class="flex flex-col gap-2 border-l pl-4">
            <TaskCommentItem
              v-for="reply in thread.replies"
              :key="reply.id"
              :comment="reply"
              :updating="updatingCommentId === reply.id"
              :deleting="deletingCommentId === reply.id"
              :on-update="(body) => onUpdate(reply.id, body)"
              :on-delete="() => onDelete(reply.id)"
            />
          </div>

          <div v-if="replyTargetId === thread.id" class="flex flex-col gap-2 border-l pl-4">
            <Textarea
              v-model="replyDraft"
              class="min-h-20 text-sm"
              :disabled="submitPending"
              aria-label="返信"
            />
            <div class="flex justify-end gap-2">
              <Button
                type="button"
                variant="outline"
                size="sm"
                class="h-7 px-2"
                :disabled="submitPending"
                @click="cancelReply"
              >
                キャンセル
              </Button>
              <Button
                type="button"
                size="sm"
                class="h-7 px-2"
                :disabled="submitPending || !replyDraft.trim()"
                @click="submitReply(thread.id)"
              >
                {{ submitPending ? '送信中…' : '返信する' }}
              </Button>
            </div>
          </div>
          <div v-else class="flex justify-start">
            <Button
              type="button"
              variant="ghost"
              size="sm"
              class="h-6 px-2 text-xs text-muted-foreground"
              :disabled="submitPending"
              @click="openReply(thread.id)"
            >
              返信
            </Button>
          </div>
        </li>
      </ul>

      <p v-if="updateError" class="mt-2 text-xs text-destructive">{{ updateError }}</p>
      <p v-if="deleteError" class="mt-2 text-xs text-destructive">{{ deleteError }}</p>

      <form class="mt-4 flex flex-col gap-2 border-t pt-4" @submit.prevent="submitNewComment">
        <Textarea
          v-model="newDraft"
          class="min-h-24 text-sm"
          :disabled="submitPending"
          aria-label="コメントを入力"
          placeholder="コメントを入力…"
        />
        <p v-if="submitError" class="text-xs text-destructive">{{ submitError }}</p>
        <div class="flex justify-end">
          <Button type="submit" size="sm" :disabled="submitPending || !newDraft.trim()">
            {{ submitPending ? '送信中…' : 'コメントする' }}
          </Button>
        </div>
      </form>
    </template>
  </section>
</template>
