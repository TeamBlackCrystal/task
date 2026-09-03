import { useMutation, useQueryClient } from '@tanstack/vue-query';
import { computed, ref, toValue, type MaybeRefOrGetter } from 'vue';

import { fetchClient } from '@/lib/api-vue-query';
import type { components } from '@/generated/api';

const GET_TASK_PATH = '/v1/tenants/{tenant_id}/projects/{project_id}/tasks/{id}' as const;
const LIST_TASKS_PATH = '/v1/tenants/{tenant_id}/projects/{project_id}/tasks' as const;
const TASK_SEARCH_PATH = '/v1/tenants/{tenant_id}/projects/{project_id}/tasks/search' as const;
const ASSIGNEES_PATH =
  '/v1/tenants/{tenant_id}/projects/{project_id}/tasks/{id}/assignees' as const;
const ASSIGNEE_PATH =
  '/v1/tenants/{tenant_id}/projects/{project_id}/tasks/{id}/assignees/{user_id}' as const;
const COMMENTS_PATH = '/v1/tenants/{tenant_id}/projects/{project_id}/tasks/{id}/comments' as const;

type TaskResponse = components['schemas']['TaskResponse'];
type UpdateTaskRequest = components['schemas']['UpdateTaskRequest'];

/** 一覧の行から直接変えられる項目。詳細を開かずに済ませたいものだけを載せる。 */
export type TaskRowField = 'status_id' | 'priority' | 'soft_deadline' | 'label_ids' | 'assignees';

export type TaskRowMutationsParams = {
  tenantId: MaybeRefOrGetter<string | null | undefined>;
  projectId: MaybeRefOrGetter<string | null | undefined>;
};

/**
 * 一覧の行からタスクを直接更新する。
 *
 * 詳細画面の {@link useTaskDetail} は「1 タスクを開いている」前提でキャッシュを
 * 持つのに対し、こちらは一覧の任意の行を対象にするので別に用意する。楽観更新は
 * 行数分の巻き戻しを持つ必要があって割に合わないため、確定後に一覧を invalidate
 * するだけにし、飛行中は行側で操作を止める（`pendingField` を見て disabled にする）。
 */
export function useTaskRowMutations(params: TaskRowMutationsParams) {
  const queryClient = useQueryClient();
  const tenantId = computed(() => toValue(params.tenantId) ?? '');
  const projectId = computed(() => toValue(params.projectId) ?? '');

  /** 更新中のタスク ID → 項目。行はこれを見て操作を止める。 */
  const pending = ref<Record<string, TaskRowField | undefined>>({});
  /** 直近の失敗。行の下に出す。 */
  const errors = ref<Record<string, string | undefined>>({});

  function pathParams(taskId: string) {
    return { tenant_id: tenantId.value, project_id: projectId.value, id: taskId };
  }

  function invalidateLists() {
    // 一覧・検索・詳細の 3 つが同じタスクを持つので、まとめて取り直す。
    // 一覧はグループごとに別クエリなので prefix 一致で全部落とす
    return Promise.all([
      queryClient.invalidateQueries({ queryKey: ['get', LIST_TASKS_PATH] }),
      queryClient.invalidateQueries({ queryKey: ['get', TASK_SEARCH_PATH] }),
      queryClient.invalidateQueries({ queryKey: ['get', GET_TASK_PATH] }),
    ]);
  }

  async function run(taskId: string, field: TaskRowField, action: () => Promise<void>) {
    if (!tenantId.value || !projectId.value) return;
    if (pending.value[taskId]) return;
    pending.value = { ...pending.value, [taskId]: field };
    errors.value = { ...errors.value, [taskId]: undefined };
    try {
      await action();
      await invalidateLists();
    } catch {
      // 失敗は行の下に出すだけ。ここで throw すると一覧全体がエラー表示になる
      errors.value = { ...errors.value, [taskId]: '更新に失敗しました' };
    } finally {
      const next = { ...pending.value };
      delete next[taskId];
      pending.value = next;
    }
  }

  async function patchTask(taskId: string, body: UpdateTaskRequest) {
    const { error } = await fetchClient.PUT(GET_TASK_PATH, {
      params: { path: pathParams(taskId) },
      body,
    });
    if (error) throw error;
  }

  function setStatus(task: TaskResponse, statusId: string) {
    if (task.status_id === statusId) return Promise.resolve();
    return run(task.id, 'status_id', () => patchTask(task.id, { status_id: statusId }));
  }

  function setPriority(task: TaskResponse, priority: TaskResponse['priority']) {
    if (task.priority === priority) return Promise.resolve();
    return run(task.id, 'priority', () => patchTask(task.id, { priority }));
  }

  /** `null` で期限を外す。API は値と clear フラグを別に持つ。 */
  function setSoftDeadline(task: TaskResponse, iso: string | null) {
    return run(task.id, 'soft_deadline', () =>
      patchTask(task.id, iso ? { soft_deadline: iso } : { clear_soft_deadline: true }),
    );
  }

  /**
   * ラベルの付け外し。
   *
   * 現在の集合はタスク自身の `labels` から取る。プロジェクトのラベル一覧は別キャッシュで
   * 古いことがあり、そちらと交差を取ると「一覧に無い = 削除済み」と誤って引き算してしまう
   * （TaskDetailHub の toggleLabel と同じ理由）。
   */
  function toggleLabel(task: TaskResponse, labelId: string, checked: boolean) {
    const current = task.labels.map((label) => label.id);
    if (checked === current.includes(labelId)) return Promise.resolve();
    const next = checked ? [...current, labelId] : current.filter((id) => id !== labelId);
    return run(task.id, 'label_ids', () => patchTask(task.id, { label_ids: next }));
  }

  /**
   * 担当者の入れ替え。`UpdateTaskRequest` に assignees が無く、専用の口しかないため
   * 「今の担当を外して、新しい担当を足す」の 2 手で行う。`null` は解除。
   */
  function setAssignee(task: TaskResponse, userId: string | null) {
    const currentIds = task.assignees.map((assignee) => assignee.user.id);
    if (userId && currentIds.length === 1 && currentIds[0] === userId) return Promise.resolve();

    return run(task.id, 'assignees', async () => {
      for (const id of currentIds) {
        if (id === userId) continue;
        const { error } = await fetchClient.DELETE(ASSIGNEE_PATH, {
          params: { path: { ...pathParams(task.id), user_id: id } },
        });
        if (error) throw error;
      }
      if (userId && !currentIds.includes(userId)) {
        const { error } = await fetchClient.POST(ASSIGNEES_PATH, {
          params: { path: pathParams(task.id) },
          // role はタスク作成時と同じ既定値。レビュアー等の役割はこの口で扱わない
          body: { user_id: userId, role: 'assignee' },
        });
        if (error) throw error;
      }
    });
  }

  const addComment = useMutation({
    mutationFn: async (input: { taskId: string; body: string }) => {
      const { error } = await fetchClient.POST(COMMENTS_PATH, {
        params: { path: pathParams(input.taskId) },
        body: { body: input.body },
      });
      if (error) throw error;
    },
    onSuccess: () => invalidateLists(),
  });

  return {
    pending,
    errors,
    setStatus,
    setPriority,
    setSoftDeadline,
    toggleLabel,
    setAssignee,
    addComment,
  };
}
