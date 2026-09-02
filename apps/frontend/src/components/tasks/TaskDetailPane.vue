<script setup lang="ts">
import { X } from '@lucide/vue';
import { ref } from 'vue';

import TaskDetailHub from '@/components/tasks/TaskDetailHub.vue';
import { Button } from '@/components/ui/button';
import { useTaskDetail } from '@/composables/useTaskDetail';

const props = defineProps<{
  /** ルートの tenant セグメント（表示ID） */
  tenantDisplayId: string;
  /** プロジェクトの key */
  projectKey: string;
  /** タスク識別子（URL と同じ seq key 形式。例: "ENG-42"） */
  taskId: string;
}>();

const emit = defineEmits<{
  close: [];
}>();

const deleteDialogRef = ref<HTMLDialogElement | null>(null);

function closeDeleteDialog() {
  deleteDialogRef.value?.close();
}

const {
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
  projectMembers,
  projectMembersLoading,
  projectMembersError,
  assigneesUpdating,
  assigneesError,
  onToggleAssignee,
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
});

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
    <div class="min-h-0 flex-1 overflow-y-auto px-4 py-4 lg:px-6">
      <TaskDetailHub
        layout="pane"
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
        :project-members="projectMembers"
        :project-members-loading="projectMembersLoading"
        :project-members-error="projectMembersError"
        :assignees-updating="assigneesUpdating"
        :assignees-error="assigneesError"
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
        @toggle:assignee="onToggleAssignee"
        :delete-disabled="deletePending"
        @delete-request="openDeleteDialog"
      >
        <template #header-actions>
          <Button
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
