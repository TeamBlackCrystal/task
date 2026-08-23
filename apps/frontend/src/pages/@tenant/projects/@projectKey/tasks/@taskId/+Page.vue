<script setup lang="ts">
import { computed, inject, ref } from 'vue';
import { navigate } from 'vike/client/router';
import { useData } from 'vike-vue/useData';
import { usePageContext } from 'vike-vue/usePageContext';

import TaskDetailHub from '@/components/tasks/TaskDetailHub.vue';
import { Button } from '@/components/ui/button';
import { useTaskDetail } from '@/composables/useTaskDetail';
import type { Data } from './+data';
import { refreshTaskDescription } from './task-description-navigation';

const pageContext = usePageContext();
// サーバ (+data.ts) が renderDescription した説明 HTML。クライアントは受けて v-html
// するだけで、再パースしない (SSR 契約: @/lib/markup-renderer/index.ts)。
// Storybook は +Page.vue を直接マウントし data を provide しないため undefined になる。
// 型で undefined を認め、参照側で null へ倒す（descriptionHtml は元々 null 許容で、
// null ならプレーンテキスト表示へフォールバックする）。
const data = useData<Data>() as Data | undefined;

// 削除後遷移の seam。既定は SPA 遷移だが、テスト等が差し替えられるよう inject 経由にする。
const navigateAfterDelete = inject<(href: string) => void>('navigateAfterDelete', (href) => {
  void navigate(href);
});

const tenantDisplayId = computed(() => String(pageContext.routeParams.tenant ?? ''));
const projectKey = computed(() => String(pageContext.routeParams.projectKey ?? ''));
const taskId = computed(() => String(pageContext.routeParams.taskId ?? ''));

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
  labelsUpdating,
  labelsError,
  fieldUpdating,
  fieldErrors,
  isLoading,
  isNotFound,
  isError,
  onStatusChange,
  onSaveTitle,
  onSaveDescription,
  onSaveProgressPct,
  onSaveSoftDeadline,
  onSaveHardDeadline,
  onSaveLabels,
  deleteError,
  deletePending,
  confirmDelete,
  listHref,
} = useTaskDetail({
  tenantDisplayId,
  projectKey,
  taskId,
  onAfterDelete: (href) => {
    closeDeleteDialog();
    navigateAfterDelete(href);
  },
  // 説明の保存直後、クライアントは生テキストしか持たない。クライアントで KFM を
  // 描画せず (バンドル退行 +417.5 KB の再来防止)、+data.ts をサーバで再実行して
  // 描画済み HTML を取り直す。保存確定後に同じ URL へ再ナビゲートするため、backend は
  // 既に新しい本文を返す。keepScrollPosition と overwriteLastHistoryEntry で
  // 長い本文の編集位置を維持しつつブラウザ履歴を増やさない。
  // 再ナビゲート失敗は黙殺しない: 保存済みデータは失われず、descriptionSource の
  // 照合不一致でプレーンテキスト表示へ倒れる (SSR 契約) ため UI エラーにはしないが、
  // 「KFM 表示に戻らない」調査の手がかりとして記録は残す。
  onAfterFieldSaved: (field) => {
    if (field === 'description') {
      refreshTaskDescription().catch((error: unknown) => {
        console.error('説明保存後の再読み込みに失敗 (プレーンテキスト表示のまま):', error);
      });
    }
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
  <TaskDetailHub
    :task="displayTask"
    :description-html="data?.descriptionHtml ?? null"
    :description-source="data?.descriptionSource ?? null"
    :project-key="projectKey"
    :statuses="statuses"
    :project-labels="projectLabels"
    :project-labels-loading="projectLabelsLoading"
    :project-labels-error="projectLabelsError"
    :status-id="selectedStatusId"
    :status-updating="statusUpdating"
    :status-error="statusError"
    :labels-updating="labelsUpdating"
    :labels-error="labelsError"
    :field-updating="fieldUpdating"
    :field-errors="fieldErrors"
    :loading="isLoading"
    :not-found="isNotFound"
    :error="isError"
    @update:status-id="onStatusChange"
    @save:title="onSaveTitle"
    @save:description="onSaveDescription"
    @save:progress_pct="onSaveProgressPct"
    @save:soft_deadline="onSaveSoftDeadline"
    @save:hard_deadline="onSaveHardDeadline"
    @save:label_ids="onSaveLabels"
    :delete-disabled="deletePending"
    @delete-request="openDeleteDialog"
  >
    <template #breadcrumb>
      <a :href="listHref" class="text-primary hover:underline">タスク一覧</a>
      <span aria-hidden="true">/</span>
    </template>
    <template #header-actions>
      <dialog
        ref="deleteDialogRef"
        class="fixed top-1/2 left-1/2 w-[calc(100%-2rem)] max-w-md -translate-x-1/2 -translate-y-1/2 rounded-lg border bg-background p-6 shadow-lg backdrop:bg-black/50 open:flex open:flex-col open:gap-4"
        aria-labelledby="delete-task-dialog-title"
        @cancel="onDeleteDialogCancel"
      >
        <h2 id="delete-task-dialog-title" class="text-lg font-semibold">タスクを削除しますか？</h2>
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
    <template #footer>
      <p class="text-xs text-muted-foreground">
        このページはタスク詳細ハブの増分2です。タイトル・説明・進捗・期限をインライン編集できます。
      </p>
    </template>
  </TaskDetailHub>
</template>
