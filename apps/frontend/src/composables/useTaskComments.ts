import { computed, ref, type MaybeRefOrGetter, toValue } from 'vue';
import { useQuery, useQueryClient } from '@tanstack/vue-query';

import { fetchClient, apiClient } from '@/lib/api-vue-query';
import type { components } from '@/generated/api';

const LIST_COMMENTS_PATH =
  '/v1/tenants/{tenant_id}/projects/{project_id}/tasks/{id}/comments' as const;
const COMMENT_PATH =
  '/v1/tenants/{tenant_id}/projects/{project_id}/tasks/{id}/comments/{cid}' as const;

export type CommentThread = components['schemas']['CommentThread'];
export type CommentReply = components['schemas']['CommentReply'];

/**
 * API のエラーボディ（{ message }）をそのまま利用者へ見せるための整形。
 * 権限判定は backend が唯一の判断者であり、frontend は拒否理由を推測せず
 * 拒まれた通りに表示する。message が取れない失敗（ネットワーク断など）は
 * 日本語の概要だけを出す。
 */
function apiErrorMessage(error: unknown, summary: string): string {
  // apiClient.useMutation の reject は OpenApiVueQueryError で、解析済みの
  // エラーボディは .error に入る（.message はラッパーの定型文なので見ない）
  const body =
    error && typeof error === 'object' && 'error' in error
      ? (error as { error: unknown }).error
      : null;
  if (
    body &&
    typeof body === 'object' &&
    'message' in body &&
    typeof (body as { message: unknown }).message === 'string' &&
    (body as { message: string }).message
  ) {
    return `${summary}（${(body as { message: string }).message}）`;
  }
  return summary;
}

export interface UseTaskCommentsParams {
  /** 解決済みテナント UUID（useTaskDetail が返すもの）。null なら未取得 */
  tenantId: MaybeRefOrGetter<string | null | undefined>;
  /** 解決済みプロジェクト UUID（useTaskDetail が返すもの）。null なら未取得 */
  projectId: MaybeRefOrGetter<string | null | undefined>;
  /** タスク識別子（URL と同じ seq key 形式。例: "ENG-42"）。空文字なら未取得 */
  taskId: MaybeRefOrGetter<string>;
}

/**
 * タスクコメント（一覧・投稿・編集・削除）のロジック。
 * useTaskDetail と同じ流儀: vue-query の一覧 query + apiClient.useMutation、
 * mutateAsync の Promise 側でキャッシュ更新（query key は開始時点で固定）、
 * 失敗はエラー ref に積んで表示側へ渡す。
 * コメントは client 取得のため、失敗してもページ本体（タスク詳細）には影響しない。
 */
export function useTaskComments(params: UseTaskCommentsParams) {
  const tenantId = computed(() => toValue(params.tenantId) ?? null);
  const projectId = computed(() => toValue(params.projectId) ?? null);
  const taskId = computed(() => String(toValue(params.taskId) ?? ''));

  const queryClient = useQueryClient();

  /** 新規投稿の失敗。返信の失敗は replyError に分け、それぞれのフォーム直下に出す */
  const submitError = ref<string | null>(null);
  const replyError = ref<string | null>(null);
  /** 返信に失敗したスレッドの ID。返信フォームの直下にエラーを出すために持つ */
  const replyErrorThreadId = ref<string | null>(null);
  const updateError = ref<string | null>(null);
  const deleteError = ref<string | null>(null);
  /** 更新・削除の対象コメント ID（進行中のみ非 null）。ボタンの disabled に使う */
  const updatingCommentId = ref<string | null>(null);
  const deletingCommentId = ref<string | null>(null);
  /** 更新・削除に失敗したコメントの ID。エラーを当該コメントの中に出すために持つ */
  const updateErrorCommentId = ref<string | null>(null);
  const deleteErrorCommentId = ref<string | null>(null);

  const commentsQueryKey = computed(
    () =>
      [
        'get',
        LIST_COMMENTS_PATH,
        {
          params: {
            path: {
              tenant_id: tenantId.value!,
              project_id: projectId.value!,
              id: taskId.value,
            },
          },
        },
      ] as const,
  );

  const commentsQuery = useQuery({
    queryKey: commentsQueryKey,
    queryFn: async ({ signal }) => {
      const { data, error } = await fetchClient.GET(LIST_COMMENTS_PATH, {
        params: {
          path: {
            tenant_id: tenantId.value!,
            project_id: projectId.value!,
            id: taskId.value,
          },
        },
        signal,
      });
      if (error) throw error;
      return data;
    },
    enabled: computed(() => !!tenantId.value && !!projectId.value && !!taskId.value),
  });

  const createCommentMutation = apiClient.useMutation('post', LIST_COMMENTS_PATH);
  const updateCommentMutation = apiClient.useMutation('put', COMMENT_PATH);
  const deleteCommentMutation = apiClient.useMutation('delete', COMMENT_PATH);

  function taskPathParams() {
    return {
      tenant_id: tenantId.value!,
      project_id: projectId.value!,
      id: taskId.value,
    };
  }

  /**
   * 投稿。parentCommentId を渡すとそのスレッドへの返信になる。
   * 成功で true（呼び出し側が下書きを消してよい）、失敗で false（下書きを残す）。
   */
  async function submitComment(body: string, parentCommentId?: string | null): Promise<boolean> {
    if (!tenantId.value || !projectId.value || !taskId.value) return false;
    submitError.value = null;
    replyError.value = null;
    replyErrorThreadId.value = null;
    // ペイン切替などで unmount しても完走した結果を対象タスクのキャッシュへ書けるよう、
    // query key は開始時点で固定する（useTaskDetail.mutateTask と同じ理由）
    const queryKey = commentsQueryKey.value;
    try {
      await createCommentMutation.mutateAsync({
        params: { path: taskPathParams() },
        body: { body, parent_comment_id: parentCommentId ?? null },
      });
      // 作成レスポンス（TaskCommentResponse）は一覧のスレッド形と型が違い
      // user 情報も含まないため、一覧を invalidate して取り直す
      await queryClient.invalidateQueries({ queryKey });
      return true;
    } catch (error) {
      // 失敗の出し先をフォームごとに分ける: 返信の失敗を最下部の新規投稿
      // フォームに出すと、失敗した場所から離れて気づけない
      if (parentCommentId) {
        replyError.value = apiErrorMessage(error, '返信を投稿できませんでした');
        replyErrorThreadId.value = parentCommentId;
      } else {
        submitError.value = apiErrorMessage(error, 'コメントを投稿できませんでした');
      }
      return false;
    }
  }

  /** 編集。成功で true（呼び出し側が編集 UI を閉じてよい）、失敗で false。 */
  async function updateComment(commentId: string, body: string): Promise<boolean> {
    if (!tenantId.value || !projectId.value || !taskId.value) return false;
    updateError.value = null;
    updateErrorCommentId.value = null;
    updatingCommentId.value = commentId;
    const queryKey = commentsQueryKey.value;
    try {
      await updateCommentMutation.mutateAsync({
        params: { path: { ...taskPathParams(), cid: commentId } },
        body: { body },
      });
      await queryClient.invalidateQueries({ queryKey });
      return true;
    } catch (error) {
      updateError.value = apiErrorMessage(error, 'コメントを更新できませんでした');
      updateErrorCommentId.value = commentId;
      return false;
    } finally {
      if (updatingCommentId.value === commentId) updatingCommentId.value = null;
    }
  }

  /** 削除。成功で true、失敗で false。 */
  async function deleteComment(commentId: string): Promise<boolean> {
    if (!tenantId.value || !projectId.value || !taskId.value) return false;
    deleteError.value = null;
    deleteErrorCommentId.value = null;
    deletingCommentId.value = commentId;
    const queryKey = commentsQueryKey.value;
    try {
      await deleteCommentMutation.mutateAsync({
        params: { path: { ...taskPathParams(), cid: commentId } },
      });
      await queryClient.invalidateQueries({ queryKey });
      return true;
    } catch (error) {
      deleteError.value = apiErrorMessage(error, 'コメントを削除できませんでした');
      deleteErrorCommentId.value = commentId;
      return false;
    } finally {
      if (deletingCommentId.value === commentId) deletingCommentId.value = null;
    }
  }

  return {
    threads: computed<CommentThread[]>(() => commentsQuery.data.value?.comments ?? []),
    commentsLoading: computed(() => commentsQuery.isLoading.value),
    commentsError: computed(() => commentsQuery.isError.value),
    /** 一覧の再試行。エラー表示の「再試行」導線から呼ぶ */
    refetchComments: () => {
      void commentsQuery.refetch();
    },
    submitPending: computed(() => createCommentMutation.isPending.value),
    submitError,
    replyError,
    replyErrorThreadId,
    updatingCommentId,
    updateError,
    updateErrorCommentId,
    deletingCommentId,
    deleteError,
    deleteErrorCommentId,
    submitComment,
    updateComment,
    deleteComment,
  };
}
