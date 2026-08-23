import { describe, it, expect, vi, beforeEach } from 'vitest';
import { defineComponent } from 'vue';
import { mount, flushPromises } from '@vue/test-utils';
import { VueQueryPlugin, QueryClient } from '@tanstack/vue-query';
import type { paths } from '@/generated/api';
import { useTaskComments } from '../useTaskComments';

const TASK_SEQ_KEY = 'ENG-1';

// vi.mock の factory から参照するため hoisted に置く
const { control, requestLog, fetchMock } = vi.hoisted(() => {
  const TASK_SEQ_KEY = 'ENG-1';

  function jsonResponse(body: unknown, status = 200) {
    return new Response(JSON.stringify(body), {
      status,
      headers: { 'Content-Type': 'application/json' },
    });
  }

  // 応答モードをテストごとに切り替える。reject は API 拒否 ({ message }) を返す
  const control: {
    listMode: 'success' | 'error';
    threads: Record<string, unknown>[];
    rejectPost?: { status: number; message: string };
    rejectPut?: { status: number; message: string };
    rejectDelete?: { status: number; message: string };
  } = { listMode: 'success', threads: [] };

  const requestLog: { method: string; url: string; body?: unknown }[] = [];

  const fetchMock = async (input: Request) => {
    const url = input.url;
    const method = input.method.toUpperCase();
    const entry: { method: string; url: string; body?: unknown } = { method, url };
    if (method === 'POST' || method === 'PUT') entry.body = await input.clone().json();
    requestLog.push(entry);

    if (method === 'GET' && url.endsWith(`/tasks/${TASK_SEQ_KEY}/comments`)) {
      if (control.listMode === 'error') return jsonResponse({ message: 'boom' }, 500);
      return jsonResponse({ comments: control.threads });
    }
    if (method === 'POST' && url.endsWith(`/tasks/${TASK_SEQ_KEY}/comments`)) {
      if (control.rejectPost) {
        return jsonResponse({ message: control.rejectPost.message }, control.rejectPost.status);
      }
      return jsonResponse(
        {
          id: 'c-new',
          task_id: 'task-1',
          user_id: 'user-1',
          body: (entry.body as { body: string }).body,
          parent_comment_id: (entry.body as { parent_comment_id?: string | null })
            .parent_comment_id,
          created_at: '2026-08-20T00:00:00Z',
          updated_at: '2026-08-20T00:00:00Z',
        },
        201,
      );
    }
    if (method === 'PUT' && /\/comments\/[^/]+$/.test(url)) {
      if (control.rejectPut) {
        return jsonResponse({ message: control.rejectPut.message }, control.rejectPut.status);
      }
      return jsonResponse({
        id: url.split('/').pop(),
        task_id: 'task-1',
        user_id: 'user-1',
        body: (entry.body as { body: string }).body,
        parent_comment_id: null,
        created_at: '2026-08-20T00:00:00Z',
        updated_at: '2026-08-20T01:00:00Z',
      });
    }
    if (method === 'DELETE' && /\/comments\/[^/]+$/.test(url)) {
      if (control.rejectDelete) {
        return jsonResponse({ message: control.rejectDelete.message }, control.rejectDelete.status);
      }
      // 本番の delete_comment は soft-delete: 行は残り、一覧では
      // is_deleted=true / body=null になる（comment_body が None を返す）。
      // モックも同じ形に倒し、削除済み表示経路を invalidate 後の一覧で踏めるようにする
      const cid = url.split('/').pop();
      for (const thread of control.threads) {
        if (thread.id === cid) {
          thread.is_deleted = true;
          thread.body = null;
        }
        for (const reply of (thread.replies as Record<string, unknown>[] | undefined) ?? []) {
          if (reply.id === cid) {
            reply.is_deleted = true;
            reply.body = null;
          }
        }
      }
      return new Response(null, { status: 204 });
    }
    return jsonResponse({ message: 'not found' }, 404);
  };

  return { control, requestLog, fetchMock };
});

vi.mock('@/lib/api-vue-query', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api-vue-query')>();
  const { default: createFetchClient } = await import('openapi-fetch');
  const { createClient } = await import('@koyori-app/openapi-vue-query');
  const testFetchClient = createFetchClient<paths>({
    baseUrl: 'http://test.local/api',
    fetch: (req: Request) => fetchMock(req),
  });
  return {
    ...actual,
    fetchClient: testFetchClient,
    apiClient: createClient<paths>(testFetchClient),
  };
});

const sampleUser = { id: 'user-1', name: '田中太郎' };

function sampleThread(id: string, body: string, replies: Record<string, unknown>[] = []) {
  return {
    id,
    body,
    is_deleted: false,
    created_at: '2026-08-19T00:00:00Z',
    updated_at: '2026-08-19T00:00:00Z',
    user: sampleUser,
    replies,
  };
}

describe('useTaskComments', () => {
  let queryClient: QueryClient;
  let comments: ReturnType<typeof useTaskComments>;

  function mountHost() {
    const Host = defineComponent({
      setup() {
        comments = useTaskComments({
          tenantId: 'tenant-1',
          projectId: 'project-1',
          taskId: TASK_SEQ_KEY,
        });
        return () => null;
      },
    });
    return mount(Host, {
      global: { plugins: [[VueQueryPlugin, { queryClient }]] },
    });
  }

  beforeEach(() => {
    queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    control.listMode = 'success';
    control.threads = [];
    control.rejectPost = undefined;
    control.rejectPut = undefined;
    control.rejectDelete = undefined;
    requestLog.length = 0;
  });

  it('一覧を取得して threads として公開する', async () => {
    control.threads = [sampleThread('c-1', '最初のコメント')];
    mountHost();
    await flushPromises();

    expect(comments.commentsLoading.value).toBe(false);
    expect(comments.commentsError.value).toBe(false);
    expect(comments.threads.value).toHaveLength(1);
    expect(comments.threads.value[0].body).toBe('最初のコメント');
  });

  it('一覧の取得失敗は commentsError として公開する（threads は空のまま）', async () => {
    control.listMode = 'error';
    mountHost();
    await flushPromises();

    expect(comments.commentsError.value).toBe(true);
    expect(comments.threads.value).toEqual([]);
  });

  it('投稿は POST body を送り、成功で true を返して一覧を取り直す', async () => {
    mountHost();
    await flushPromises();

    control.threads = [sampleThread('c-new', '新しいコメント')];
    const posted = await comments.submitComment('新しいコメント');
    await flushPromises();

    expect(posted).toBe(true);
    const post = requestLog.find((r) => r.method === 'POST');
    expect(post?.body).toEqual({ body: '新しいコメント', parent_comment_id: null });
    // invalidate による取り直しで新しい一覧が threads へ反映される
    expect(comments.threads.value.map((t) => t.id)).toEqual(['c-new']);
  });

  it('返信は parent_comment_id を付けて POST する', async () => {
    control.threads = [sampleThread('c-1', '親コメント')];
    mountHost();
    await flushPromises();

    const posted = await comments.submitComment('返信です', 'c-1');
    await flushPromises();

    expect(posted).toBe(true);
    const post = requestLog.find((r) => r.method === 'POST');
    expect(post?.body).toEqual({ body: '返信です', parent_comment_id: 'c-1' });
  });

  it('投稿が API に拒否されたら false を返し、サーバの message を拒まれた通りに見せる', async () => {
    control.rejectPost = { status: 403, message: 'forbidden' };
    mountHost();
    await flushPromises();

    const posted = await comments.submitComment('拒否されるコメント');

    expect(posted).toBe(false);
    expect(comments.submitError.value).toBe('コメントを投稿できませんでした（forbidden）');
    // 新規投稿の失敗は返信側のエラーには入らない
    expect(comments.replyError.value).toBe(null);
    expect(comments.replyErrorThreadId.value).toBe(null);
  });

  it('返信が API に拒否されたら replyError と対象スレッド ID に入れる（submitError には入れない）', async () => {
    control.threads = [sampleThread('c-1', '親コメント')];
    control.rejectPost = { status: 400, message: 'thread deleted' };
    mountHost();
    await flushPromises();

    const posted = await comments.submitComment('返信です', 'c-1');

    expect(posted).toBe(false);
    expect(comments.replyError.value).toBe('返信を投稿できませんでした（thread deleted）');
    expect(comments.replyErrorThreadId.value).toBe('c-1');
    expect(comments.submitError.value).toBe(null);
  });

  it('clearReplyError は返信失敗の表示を消す', async () => {
    control.threads = [sampleThread('c-1', '親コメント')];
    control.rejectPost = { status: 400, message: 'thread deleted' };
    mountHost();
    await flushPromises();

    await comments.submitComment('返信です', 'c-1');
    expect(comments.replyError.value).not.toBe(null);

    comments.clearReplyError();
    expect(comments.replyError.value).toBe(null);
    expect(comments.replyErrorThreadId.value).toBe(null);
  });

  it('編集は PUT body を送り、成功で true を返して一覧を取り直す', async () => {
    control.threads = [sampleThread('c-1', '元の本文')];
    mountHost();
    await flushPromises();

    control.threads = [sampleThread('c-1', '直した本文')];
    const saved = await comments.updateComment('c-1', '直した本文');
    await flushPromises();

    expect(saved).toBe(true);
    expect(comments.updatingCommentId.value).toBe(null);
    const put = requestLog.find((r) => r.method === 'PUT');
    expect(put?.url.endsWith('/comments/c-1')).toBe(true);
    expect(put?.body).toEqual({ body: '直した本文' });
    expect(comments.threads.value[0].body).toBe('直した本文');
  });

  it('編集が API に拒否されたら false を返し、サーバの message を見せる', async () => {
    control.threads = [sampleThread('c-1', '元の本文')];
    control.rejectPut = { status: 403, message: 'not your comment' };
    mountHost();
    await flushPromises();

    const saved = await comments.updateComment('c-1', '直した本文');

    expect(saved).toBe(false);
    expect(comments.updateError.value).toBe('コメントを更新できませんでした（not your comment）');
    // 進行中 ID は消えるが、エラーの対象 ID は表示のために残る
    expect(comments.updatingCommentId.value).toBe(null);
    expect(comments.updateErrorCommentId.value).toBe('c-1');
  });

  it('clearUpdateError は編集失敗の表示を消す（clearReplyError と同型）', async () => {
    control.threads = [sampleThread('c-1', '元の本文')];
    control.rejectPut = { status: 403, message: 'not your comment' };
    mountHost();
    await flushPromises();

    await comments.updateComment('c-1', '直した本文');
    expect(comments.updateError.value).not.toBe(null);

    comments.clearUpdateError();
    expect(comments.updateError.value).toBe(null);
    expect(comments.updateErrorCommentId.value).toBe(null);
  });

  it('clearDeleteError は削除失敗の表示を消す（clearReplyError と同型）', async () => {
    control.threads = [sampleThread('c-1', '消せないコメント')];
    control.rejectDelete = { status: 403, message: 'forbidden' };
    mountHost();
    await flushPromises();

    await comments.deleteComment('c-1');
    expect(comments.deleteError.value).not.toBe(null);

    comments.clearDeleteError();
    expect(comments.deleteError.value).toBe(null);
    expect(comments.deleteErrorCommentId.value).toBe(null);
  });

  it('削除は DELETE を送り、成功で true を返して一覧を取り直す（soft-delete の形で残る）', async () => {
    control.threads = [sampleThread('c-1', '消すコメント')];
    mountHost();
    await flushPromises();

    const deleted = await comments.deleteComment('c-1');
    await flushPromises();

    expect(deleted).toBe(true);
    expect(comments.deletingCommentId.value).toBe(null);
    const del = requestLog.find((r) => r.method === 'DELETE');
    expect(del?.url.endsWith('/comments/c-1')).toBe(true);
    // 本番は行を消さず soft-delete するため、取り直した一覧にも
    // is_deleted=true / body=null の形で残る（削除済み表示経路の前提）
    expect(comments.threads.value).toHaveLength(1);
    expect(comments.threads.value[0].is_deleted).toBe(true);
    expect(comments.threads.value[0].body).toBe(null);
  });

  it('削除が API に拒否されたら false を返し、サーバの message を見せる', async () => {
    control.threads = [sampleThread('c-1', '消せないコメント')];
    control.rejectDelete = { status: 403, message: 'forbidden' };
    mountHost();
    await flushPromises();

    const deleted = await comments.deleteComment('c-1');

    expect(deleted).toBe(false);
    expect(comments.deleteError.value).toBe('コメントを削除できませんでした（forbidden）');
    expect(comments.deletingCommentId.value).toBe(null);
    expect(comments.deleteErrorCommentId.value).toBe('c-1');
  });
});
