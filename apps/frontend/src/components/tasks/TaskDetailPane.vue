<script setup lang="ts">
import { X } from '@lucide/vue';
import { computed, ref } from 'vue';

import TaskActivityFeed from '@/components/tasks/TaskActivityFeed.vue';
import TaskComments from '@/components/tasks/TaskComments.vue';
import TaskDetailHub from '@/components/tasks/TaskDetailHub.vue';
import { Button } from '@/components/ui/button';
import { useAssignableUsersQuery } from '@/lib/api-vue-query';
import { useTaskRowMutations } from '@/composables/useTaskRowMutations';
import { useTaskActivities } from '@/composables/useTaskActivities';
import { useTaskComments } from '@/composables/useTaskComments';
import { useTaskDetail } from '@/composables/useTaskDetail';
import { useMeQuery } from '@/lib/api-vue-query';

const props = withDefaults(
  defineProps<{
    /** ルートの tenant セグメント（表示ID） */
    tenantDisplayId: string;
    /** プロジェクトの key */
    projectKey: string;
    /** タスク識別子（URL と同じ seq key 形式。例: "ENG-42"） */
    taskId: string;
    /**
     * 詳細の並び。分割ビューは幅が狭いので 1 カラム（`pane`）、
     * オーバーレイは広いので詳細ページと同じ 3 カラム（`page`）にする。
     */
    layout?: 'page' | 'pane';
    /** 器が閉じる手段を持つ場合（オーバーレイの × など）は自前の閉じるボタンを出さない */
    showCloseButton?: boolean;
    /** 一覧の cursor chain を捨てて先頭から読み直す。 */
    onTaskListChanged?: () => void;
  }>(),
  { layout: 'pane', showCloseButton: true },
);

const emit = defineEmits<{
  close: [];
}>();

const deleteDialogRef = ref<HTMLDialogElement | null>(null);

function closeDeleteDialog() {
  deleteDialogRef.value?.close();
}

const {
  tenantId,
  projectId,
  displayTask,
  statuses,
  projectLabels,
  projectLabelsLoading,
  projectLabelsError,
  selectedStatusId,
  statusUpdating,
  statusError,
  priorityUpdating,
  priorityError,
  labelsUpdating,
  labelsError,
  fieldUpdating,
  fieldErrors,
  isLoading,
  isNotFound,
  isError,
  onStatusChange,
  onPriorityChange,
  onSaveTitle,
  onSaveDescription,
  onSaveProgressPct,
  onSaveSoftDeadline,
  onSaveHardDeadline,
  onSaveLabels,
  deleteError,
  deletePending,
  confirmDelete,
} = useTaskDetail({
  tenantDisplayId: () => props.tenantDisplayId,
  projectKey: () => props.projectKey,
  taskId: () => props.taskId,
  // 削除成功時はペインを閉じる。一覧は useTaskDetail 側の invalidate で自動更新される。
  onAfterDelete: () => {
    closeDeleteDialog();
    emit('close');
  },
  onTaskListChanged: props.onTaskListChanged,
});

// アクティビティ（コメント）。詳細ページと同じ composable を使い、
// オーバーレイでも本文と同じ画面でやり取りできるようにする
const {
  threads,
  commentsLoading,
  commentsError,
  refetchComments,
  submitPending,
  submitError,
  replyError,
  replyErrorThreadId,
  updatingCommentId,
  updateError,
  updateErrorCommentId,
  deletingCommentId,
  deleteError: commentDeleteError,
  deleteErrorCommentId,
  clearReplyError,
  clearUpdateError,
  clearDeleteError,
  submitComment,
  updateComment,
  deleteComment,
} = useTaskComments({
  tenantId,
  projectId,
  taskId: computed(() => props.taskId),
});

// 履歴はコメントと別の口。片方が落ちてももう片方は読み書きできる
const {
  activities,
  activitiesLoading,
  activitiesError,
  hasMoreActivities,
  activitiesFetchingMore,
  loadMoreActivities,
  refetchActivities,
} = useTaskActivities({
  tenantId,
  projectId,
  taskId: computed(() => props.taskId),
});

// 担当者の割り当て。詳細でも一覧の行と同じ口（専用の POST/DELETE）を使う
const membersQuery = useAssignableUsersQuery(tenantId, projectId);
const members = computed(() => membersQuery.data.value ?? []);

// 候補の取得状態はピッカーへ渡す（取得中・失敗を「候補 0 人」と混ぜない）
const membersState = computed(() => ({
  loading: membersQuery.isLoading.value,
  error: membersQuery.isError.value,
  onRetry: () => void membersQuery.refetch(),
}));
const rowMutations = useTaskRowMutations({ tenantId, projectId });

function onToggleAssignee(userId: string, checked: boolean) {
  const task = displayTask.value;
  if (!task) return;
  void rowMutations.toggleAssignee(task, userId, checked);
}

// 担当者の更新は行と同じ口を使うので、飛行中と失敗をこの画面にも出す
// （出さないと失敗が無表示、送信中も押せて 2 回目が無言で捨てられる）
const assigneeUpdating = computed(() => {
  const id = displayTask.value?.id;
  return !!id && rowMutations.pending.value[id] === 'assignees';
});
const assigneeError = computed(() => {
  const id = displayTask.value?.id;
  return id ? (rowMutations.errors.value[id] ?? null) : null;
});

const meQuery = useMeQuery();
const currentUserId = computed(() => meQuery.data.value?.id ?? null);

function openDeleteDialog() {
  deleteError.value = null;
  deleteDialogRef.value?.showModal();
}

function onDeleteDialogCancel(event: Event) {
  event.preventDefault();
  if (deletePending.value) return;
  closeDeleteDialog();
}
</script>

<template>
  <div class="flex h-full min-h-0 flex-col">
    <!--
      横の余白はここで付けない。アクティビティ列の境界線と入力欄の帯を
      器の端まで通すため、余白は本文・列のそれぞれが持つ（TaskDetailHub）。
    -->
    <div class="flex min-h-0 flex-1 flex-col overflow-hidden">
      <TaskDetailHub
        :layout="layout"
        :task="displayTask"
        :project-key="projectKey"
        :statuses="statuses"
        :project-labels="projectLabels"
        :project-labels-loading="projectLabelsLoading"
        :project-labels-error="projectLabelsError"
        :status-id="selectedStatusId"
        :status-updating="statusUpdating"
        :status-error="statusError"
        :priority-updating="priorityUpdating"
        :priority-error="priorityError"
        :labels-updating="labelsUpdating"
        :labels-error="labelsError"
        :field-updating="fieldUpdating"
        :field-errors="fieldErrors"
        :loading="isLoading"
        :not-found="isNotFound"
        :error="isError"
        @update:status-id="onStatusChange"
        @change:priority="onPriorityChange"
        @save:title="onSaveTitle"
        @save:description="onSaveDescription"
        @save:progress_pct="onSaveProgressPct"
        @save:soft_deadline="onSaveSoftDeadline"
        @save:hard_deadline="onSaveHardDeadline"
        @save:label_ids="onSaveLabels"
        :members="members"
        :assignee-updating="assigneeUpdating"
        :assignee-error="assigneeError"
        :members-state="membersState"
        @toggle:assignee="onToggleAssignee"
        :delete-disabled="deletePending"
        @delete-request="openDeleteDialog"
      >
        <template #sidebar>
          <TaskComments
            :threads="threads"
            :loading="commentsLoading"
            :list-error="commentsError"
            :on-retry="refetchComments"
            :current-user-id="currentUserId"
            :submit-pending="submitPending"
            :submit-error="submitError"
            :reply-error="replyError"
            :reply-error-thread-id="replyErrorThreadId"
            :updating-comment-id="updatingCommentId"
            :update-error="updateError"
            :update-error-comment-id="updateErrorCommentId"
            :deleting-comment-id="deletingCommentId"
            :delete-error="commentDeleteError"
            :delete-error-comment-id="deleteErrorCommentId"
            :on-submit="submitComment"
            :on-update="updateComment"
            :on-delete="deleteComment"
            :on-clear-reply-error="clearReplyError"
            :on-clear-update-error="clearUpdateError"
            :on-clear-delete-error="clearDeleteError"
          >
            <template #before-list>
              <TaskActivityFeed
                :activities="activities"
                :loading="activitiesLoading"
                :error="activitiesError"
                :on-retry="refetchActivities"
                :has-more="hasMoreActivities"
                :fetching-more="activitiesFetchingMore"
                :on-load-more="loadMoreActivities"
              />
            </template>
          </TaskComments>
        </template>
        <template #header-actions>
          <Button
            v-if="showCloseButton"
            type="button"
            variant="ghost"
            size="icon"
            class="size-8"
            aria-label="詳細を閉じる"
            title="閉じる"
            @click="emit('close')"
          >
            <X class="size-4" />
          </Button>
          <dialog
            ref="deleteDialogRef"
            class="fixed top-1/2 left-1/2 w-[calc(100%-2rem)] max-w-md -translate-x-1/2 -translate-y-1/2 rounded-lg border bg-background p-6 shadow-lg backdrop:bg-black/50 open:flex open:flex-col open:gap-4"
            aria-labelledby="delete-task-pane-dialog-title"
            @cancel="onDeleteDialogCancel"
          >
            <h2 id="delete-task-pane-dialog-title" class="text-lg font-semibold">
              タスクを削除しますか？
            </h2>
            <p class="text-sm text-muted-foreground">
              「{{ displayTask?.title }}」を削除します。この操作は取り消せません。
            </p>
            <p v-if="deleteError" class="text-sm text-destructive">{{ deleteError }}</p>
            <div class="flex justify-end gap-2">
              <Button
                type="button"
                variant="outline"
                :disabled="deletePending"
                @click="closeDeleteDialog"
              >
                キャンセル
              </Button>
              <Button
                type="button"
                variant="destructive"
                :disabled="deletePending"
                @click="confirmDelete"
              >
                {{ deletePending ? '削除中…' : '削除する' }}
              </Button>
            </div>
          </dialog>
        </template>
      </TaskDetailHub>
    </div>
  </div>
</template>
