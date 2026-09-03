<script setup lang="ts">
import {
  ArrowLeft,
  AtSign,
  ChevronDown,
  Loader2,
  Paperclip,
  Plus,
  SendHorizontal,
} from '@lucide/vue';
import { computed, ref, watch } from 'vue';

import TaskCommentItem from '@/components/tasks/TaskCommentItem.vue';
import { Button } from '@/components/ui/button';
import { Textarea } from '@/components/ui/textarea';
import type { CommentThread } from '@/composables/useTaskComments';

const props = defineProps<{
  threads: CommentThread[];
  /** 一覧の読み込み中 */
  loading?: boolean;
  /**
   * 一覧の読み込み失敗。コメント節の中だけで倒し、ページ本体には影響させない。
   * 投稿（POST）は一覧（GET）と独立に成功しうるため、投稿フォームは出したままにする
   */
  listError?: boolean;
  /** 一覧の再試行（listError 表示の「再試行」から呼ぶ） */
  onRetry?: () => void;
  /** ログイン中ユーザーの ID。編集ボタンの出し分けに使う（TaskCommentItem 参照） */
  currentUserId?: string | null;
  /** 投稿（新規・返信共通）の進行中 */
  submitPending?: boolean;
  /** 新規投稿の失敗。新規投稿フォームの直下に出す */
  submitError?: string | null;
  /** 返信の失敗。replyErrorThreadId のスレッドの返信フォーム直下に出す */
  replyError?: string | null;
  replyErrorThreadId?: string | null;
  /** 更新リクエスト進行中のコメント ID */
  updatingCommentId?: string | null;
  /** 更新失敗のメッセージと対象コメント ID（当該コメントの中に出す） */
  updateError?: string | null;
  updateErrorCommentId?: string | null;
  /** 削除リクエスト進行中のコメント ID */
  deletingCommentId?: string | null;
  /** 削除失敗のメッセージと対象コメント ID（当該コメントの中に出す） */
  deleteError?: string | null;
  deleteErrorCommentId?: string | null;
  /**
   * 投稿の確定（useTaskComments.submitComment）。成功で true を返したら
   * 下書きを消す。失敗時は下書きを残し、submitError が拒否理由を表示する。
   */
  onSubmit: (body: string, parentCommentId?: string | null) => Promise<boolean>;
  /** 編集の確定（useTaskComments.updateComment） */
  onUpdate: (commentId: string, body: string) => Promise<boolean>;
  /** 削除の確定（useTaskComments.deleteComment） */
  onDelete: (commentId: string) => Promise<boolean>;
  /** 返信フォームを開き直すときに前回の失敗表示を消す（useTaskComments.clearReplyError） */
  onClearReplyError?: () => void;
  /** 編集 UI の開閉で前回の失敗表示を消す（useTaskComments.clearUpdateError） */
  onClearUpdateError?: () => void;
  /** 削除確認の開閉で前回の失敗表示を消す（useTaskComments.clearDeleteError） */
  onClearDeleteError?: () => void;
}>();

const newDraft = ref('');

/**
 * 開いているスレッドの ID。
 *
 * 参照どおり、返信は一覧の中に折りたたむのではなく列ごとスレッドへ切り替える。
 * 一覧では返信件数だけを出し、押すとその親と返信だけが並ぶ。
 */
const openThreadId = ref<string | null>(null);
const openThread = computed(() => props.threads.find((thread) => thread.id === openThreadId.value));

// 開いていたスレッドが消えた（削除・再取得で不在）ら一覧へ戻す
watch(
  () => props.threads,
  (threads) => {
    if (openThreadId.value && !threads.some((thread) => thread.id === openThreadId.value)) {
      openThreadId.value = null;
    }
  },
);

/**
 * 下端の入力欄。一覧では新規コメント、スレッドを開いているときはそのスレッドへの返信。
 * 入力欄を 1 つに保つことで、書いている途中に場所を見失わない。
 */
async function submitDraft() {
  const body = newDraft.value.trim();
  if (!body || props.submitPending) return;
  const posted = openThreadId.value
    ? await props.onSubmit(body, openThreadId.value)
    : await props.onSubmit(body);
  if (posted) newDraft.value = '';
}

function showThread(threadId: string) {
  props.onClearReplyError?.();
  openThreadId.value = threadId;
  newDraft.value = '';
}

function backToList() {
  props.onClearReplyError?.();
  openThreadId.value = null;
  newDraft.value = '';
}

async function deleteThread(threadId: string) {
  const deleted = await props.onDelete(threadId);
  // 削除済みスレッドへは返信できない（backend が 400 で弾く）ので一覧へ戻す
  if (deleted && openThreadId.value === threadId) backToList();
  return deleted;
}
</script>

<template>
  <!--
    アクティビティ列にそのまま収まる形にする（枠で囲わない・見出しは列側が持つ）。
    一覧は伸びる領域として上に、入力欄は列の下端に貼り付ける。
  -->
  <section class="flex min-h-0 flex-1 flex-col" data-task-comments>
    <div v-if="loading" class="flex justify-center py-6">
      <Loader2 class="h-5 w-5 animate-spin text-muted-foreground" aria-hidden="true" />
    </div>

    <template v-else>
      <!-- 一覧の失敗はリストの位置で倒し、投稿フォームは残す — 読めないことと
           書けないことを連動させない（GET と POST は backend でも独立） -->
      <div v-if="listError" class="flex items-center gap-2">
        <p class="text-sm text-destructive">コメントを読み込めませんでした</p>
        <Button
          v-if="onRetry"
          type="button"
          variant="outline"
          size="sm"
          class="h-7 px-2"
          @click="onRetry"
        >
          再試行
        </Button>
      </div>

      <template v-else>
        <!-- スレッドを開いているときは、列の頭に戻る導線を出す（参照） -->
        <div v-if="openThread" class="-mx-4 flex items-center gap-2 border-b px-4 pb-2">
          <Button
            type="button"
            variant="ghost"
            size="icon"
            class="size-7"
            aria-label="コメント一覧へ戻る"
            @click="backToList"
          >
            <ArrowLeft class="size-4" aria-hidden="true" />
          </Button>
          <p class="min-w-0 truncate text-sm font-medium">{{ openThread.user.name }} のスレッド</p>
        </div>

        <!--
          履歴とコメントを同じスクロール領域に入れ、少ないときは下端へ寄せる（参照）。
          帯は薄いグレーにして、コメントだけ白いカードで浮かせる。
        -->
        <div
          class="-mx-4 flex min-h-0 flex-1 flex-col justify-end gap-3 overflow-y-auto bg-muted/30 px-4 py-3"
        >
          <slot v-if="!openThread" name="before-list" />

          <p v-if="!threads.length" class="text-sm text-muted-foreground">
            コメントはまだありません
          </p>

          <!-- スレッド表示。参照どおり列ごと切り替える -->
          <template v-else-if="openThread">
            <div class="flex flex-col gap-3">
              <div class="rounded-lg border bg-background p-3">
                <TaskCommentItem
                  :comment="openThread"
                  :current-user-id="currentUserId"
                  :updating="updatingCommentId === openThread.id"
                  :deleting="deletingCommentId === openThread.id"
                  :update-error="updateErrorCommentId === openThread.id ? updateError : null"
                  :delete-error="deleteErrorCommentId === openThread.id ? deleteError : null"
                  :on-update="(body) => onUpdate(openThread!.id, body)"
                  :on-delete="() => deleteThread(openThread!.id)"
                  :on-clear-update-error="onClearUpdateError"
                  :on-clear-delete-error="onClearDeleteError"
                />
              </div>

              <div v-if="openThread.replies.length" class="flex items-center gap-3">
                <span class="shrink-0 text-xs text-muted-foreground">
                  {{ openThread.replies.length }}件の返信
                </span>
                <span class="h-px flex-1 bg-border" aria-hidden="true" />
              </div>

              <div
                v-for="reply in openThread.replies"
                :key="reply.id"
                class="rounded-lg border bg-background p-3"
              >
                <TaskCommentItem
                  :comment="reply"
                  :current-user-id="currentUserId"
                  :updating="updatingCommentId === reply.id"
                  :deleting="deletingCommentId === reply.id"
                  :update-error="updateErrorCommentId === reply.id ? updateError : null"
                  :delete-error="deleteErrorCommentId === reply.id ? deleteError : null"
                  :on-update="(body) => onUpdate(reply.id, body)"
                  :on-delete="() => onDelete(reply.id)"
                  :on-clear-update-error="onClearUpdateError"
                  :on-clear-delete-error="onClearDeleteError"
                />
              </div>

              <p
                v-if="replyError && replyErrorThreadId === openThread.id"
                class="text-xs text-destructive"
              >
                {{ replyError }}
              </p>
            </div>
          </template>

          <!-- 一覧。返信は展開せず「N件の返信」だけ出す -->
          <ul v-else class="flex flex-col gap-3">
            <li v-for="thread in threads" :key="thread.id" class="rounded-lg border bg-background">
              <div class="p-3">
                <TaskCommentItem
                  :comment="thread"
                  :current-user-id="currentUserId"
                  :updating="updatingCommentId === thread.id"
                  :deleting="deletingCommentId === thread.id"
                  :update-error="updateErrorCommentId === thread.id ? updateError : null"
                  :delete-error="deleteErrorCommentId === thread.id ? deleteError : null"
                  :on-update="(body) => onUpdate(thread.id, body)"
                  :on-delete="() => deleteThread(thread.id)"
                  :on-clear-update-error="onClearUpdateError"
                  :on-clear-delete-error="onClearDeleteError"
                />
              </div>

              <!--
                削除済みスレッドには返信できない（backend の create_comment が 400 で弾く）が、
                既にある返信は残るので、返信があるときは開く導線を残す。ここを隠すと
                返信がデータ上は生きているのに読むことも消すこともできなくなる。
              -->
              <div
                v-if="!thread.is_deleted || thread.replies.length"
                class="flex items-center justify-end border-t px-3 py-1.5"
              >
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  class="h-7 px-2 text-xs text-muted-foreground"
                  @click="showThread(thread.id)"
                >
                  {{ thread.replies.length ? `${thread.replies.length}件の返信` : '返信' }}
                </Button>
              </div>
            </li>
          </ul>
        </div>
      </template>

      <!--
        入力欄（参照）。下端に貼り付けた薄い背景の帯の中に、白い入力枠を置く。
        枠の中は本文 → 操作行（追加・種別・添付・メンション・送信）の順。
      -->
      <p
        v-if="openThread?.is_deleted"
        class="-mx-4 mt-auto shrink-0 border-t bg-background px-4 py-3 text-sm text-muted-foreground"
      >
        削除されたコメントには返信できません
      </p>
      <form
        v-else
        class="-mx-4 mt-auto shrink-0 border-t bg-background px-4 py-3"
        @submit.prevent="submitDraft"
      >
        <div class="rounded-lg border bg-background focus-within:ring-1 focus-within:ring-ring">
          <Textarea
            v-model="newDraft"
            class="min-h-14 resize-none border-0 bg-transparent text-sm shadow-none focus-visible:ring-0"
            :disabled="submitPending"
            :aria-label="openThread ? '返信を入力' : 'コメントを入力'"
            :placeholder="openThread ? '返信を入力…' : 'コメントを入力…'"
          />
          <div class="flex items-center gap-1 px-2 pb-2">
            <Button
              type="button"
              variant="ghost"
              size="icon"
              class="size-7 text-muted-foreground"
              aria-label="追加"
              disabled
            >
              <Plus class="size-4" aria-hidden="true" />
            </Button>
            <span
              class="inline-flex h-7 items-center gap-1 rounded-md bg-muted px-2 text-xs text-muted-foreground"
            >
              コメント
              <ChevronDown class="size-3" aria-hidden="true" />
            </span>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              class="size-7 text-muted-foreground"
              aria-label="ファイルを添付"
              disabled
            >
              <Paperclip class="size-4" aria-hidden="true" />
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              class="size-7 text-muted-foreground"
              aria-label="メンション"
              disabled
            >
              <AtSign class="size-4" aria-hidden="true" />
            </Button>
            <Button
              type="submit"
              size="icon"
              class="ml-auto size-7"
              :aria-label="submitPending ? '送信中' : openThread ? '返信する' : 'コメントする'"
              :disabled="submitPending || !newDraft.trim()"
            >
              <Loader2 v-if="submitPending" class="size-4 animate-spin" aria-hidden="true" />
              <SendHorizontal v-else class="size-4" aria-hidden="true" />
            </Button>
          </div>
        </div>
        <p v-if="submitError" class="mt-1 text-xs text-destructive">{{ submitError }}</p>
      </form>
    </template>
  </section>
</template>
