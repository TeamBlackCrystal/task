import { describe, it, expect, vi, beforeEach } from 'vitest';
import { defineComponent } from 'vue';
import { mount, flushPromises } from '@vue/test-utils';
import { VueQueryPlugin, QueryClient } from '@tanstack/vue-query';
import type { paths } from '@/generated/api';
import { useTaskRowMutations } from '../useTaskRowMutations';
import { useTaskActivities } from '../useTaskActivities';
import { useTaskComments } from '../useTaskComments';

// vi.mock の factory から参照するため hoisted に置く
const { control, requestLog, fetchMock } = vi.hoisted(() => {
  function jsonResponse(body: unknown, status = 200) {
    return new Response(JSON.stringify(body), {
      status,
      headers: { 'Content-Type': 'application/json' },
    });
  }

  const control: {
    /** POST を解決させずに保留する。飛行中の挙動を観測するために使う */
    holdPost: boolean;
    /** 保留中の POST を全部通す。1 件だけ持つと、2 本目で 1 本目を取り落とす */
    held: (() => void)[];
    rejectPost: boolean;
  } = { holdPost: false, held: [], rejectPost: false };

  const requestLog: { method: string; url: string; body?: unknown }[] = [];

  const fetchMock = async (input: Request) => {
    const url = input.url;
    const method = input.method.toUpperCase();
    const entry: { method: string; url: string; body?: unknown } = { method, url };
    if (method === 'POST') entry.body = await input.clone().json();
    requestLog.push(entry);

    if (method === 'GET') {
      if (url.includes('/activities')) return jsonResponse({ activities: [], total: 0 });
      if (url.includes('/comments')) return jsonResponse({ comments: [] });
      return jsonResponse({ tasks: [], total: 0 });
    }

    if (control.holdPost) {
      await new Promise<void>((resolve) => {
        control.held.push(resolve);
      });
    }
    if (control.rejectPost) return jsonResponse({ message: 'boom' }, 500);

    if (url.endsWith('/comments')) {
      return jsonResponse({ id: 'c-new' }, 201);
    }
    return jsonResponse({ id: 'task-new', seq_id: 9 }, 201);
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

describe('useTaskRowMutations', () => {
  let queryClient: QueryClient;
  let mutations: ReturnType<typeof useTaskRowMutations>;

  /**
   * 履歴とコメント一覧も一緒にマウントする。invalidate は表示中のクエリしか
   * 取り直さないので、「行から更新したら詳細側も取り直す」の配線はこの形でしか
   * 確かめられない。
   */
  function mountHost() {
    const Host = defineComponent({
      setup() {
        mutations = useTaskRowMutations({ tenantId: 'tenant-1', projectId: 'project-1' });
        useTaskActivities({ tenantId: 'tenant-1', projectId: 'project-1', taskId: 'task-1' });
        useTaskComments({ tenantId: 'tenant-1', projectId: 'project-1', taskId: 'task-1' });
        return () => null;
      },
    });
    return mount(Host, { global: { plugins: [[VueQueryPlugin, { queryClient }]] } });
  }

  function activityRequests() {
    return requestLog.filter(
      (entry) => entry.method === 'GET' && entry.url.includes('/activities'),
    );
  }

  function commentRequests() {
    return requestLog.filter((entry) => entry.method === 'GET' && entry.url.includes('/comments'));
  }

  beforeEach(() => {
    queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    control.holdPost = false;
    control.held = [];
    control.rejectPost = false;
    requestLog.length = 0;
  });

  it('コメント追加の失敗を行のエラーとして残し、false を返す', async () => {
    mountHost();
    control.rejectPost = true;

    await expect(mutations.addComment('task-1', 'だめなコメント')).resolves.toBe(false);
    expect(mutations.errors.value['task-1']).toBe('コメントを追加できませんでした');
  });

  // 飛行中の抑止を 1 件しか持たないと、行 A の送信中に行 B から送ったぶんが
  // 「押せるのに何も起きない」形で落ちる（無効化は対象単位なので B は押せる）
  it('別のタスクへのコメントは、送信中のタスクがあっても落とさない', async () => {
    mountHost();
    control.holdPost = true;

    const first = mutations.addComment('task-1', 'A のコメント');
    await flushPromises();
    expect(mutations.commentPendingTaskIds.value['task-1']).toBe(true);

    const second = mutations.addComment('task-2', 'B のコメント');
    await flushPromises();
    expect(mutations.commentPendingTaskIds.value['task-2']).toBe(true);

    control.holdPost = false;
    for (const release of control.held) release();
    await expect(first).resolves.toBe(true);
    await expect(second).resolves.toBe(true);

    const posted = requestLog.filter((entry) => entry.method === 'POST');
    expect(posted).toHaveLength(2);
    expect(posted.map((entry) => (entry.body as { body: string }).body)).toEqual([
      'A のコメント',
      'B のコメント',
    ]);
  });

  it('同じタスクへの二重送信は落とす', async () => {
    mountHost();
    control.holdPost = true;

    const first = mutations.addComment('task-1', '1 回目');
    await flushPromises();
    await expect(mutations.addComment('task-1', '2 回目')).resolves.toBe(false);

    control.holdPost = false;
    for (const release of control.held) release();
    await expect(first).resolves.toBe(true);
    expect(requestLog.filter((entry) => entry.method === 'POST')).toHaveLength(1);
  });

  it('別のグループへの作成は、作成中のグループがあっても落とさない', async () => {
    mountHost();
    control.holdPost = true;

    const first = mutations.createTask({ title: 'A', statusId: 'status-a' });
    await flushPromises();
    const second = mutations.createTask({ title: 'B', statusId: 'status-b' });
    await flushPromises();

    control.holdPost = false;
    for (const release of control.held) release();
    await expect(first).resolves.toBe(true);
    await expect(second).resolves.toBe(true);
    expect(requestLog.filter((entry) => entry.method === 'POST')).toHaveLength(2);
  });

  // backend は更新のたびに task_activities を積むので、取り直さないと
  // 詳細のアクティビティ欄だけが古いまま残る（画面上は更新できているので気づけない）
  it('コメント追加のあとに履歴を取り直す', async () => {
    mountHost();
    await flushPromises();
    const before = activityRequests().length;
    expect(before).toBeGreaterThan(0);

    await expect(mutations.addComment('task-1', 'コメント')).resolves.toBe(true);
    await flushPromises();

    expect(activityRequests().length).toBeGreaterThan(before);
  });

  // 行と詳細（useTaskComments）が同じコメント一覧へ書くので、行から投稿したときも
  // コメント一覧を落とさないと、詳細を開いたまま足したコメントが出てこない
  it('コメント追加のあとにコメント一覧を取り直す', async () => {
    mountHost();
    await flushPromises();
    const before = commentRequests().length;
    expect(before).toBeGreaterThan(0);

    await expect(mutations.addComment('task-1', 'コメント')).resolves.toBe(true);
    await flushPromises();

    expect(commentRequests().length).toBeGreaterThan(before);
  });

  it('作成の失敗はグループごとのエラーに入る', async () => {
    mountHost();
    control.rejectPost = true;

    await expect(mutations.createTask({ title: 'X', statusId: 'status-a' })).resolves.toBe(false);
    expect(mutations.createErrors.value['status-a']).toBe('タスクを作成できませんでした');
    expect(mutations.createErrors.value['status-b']).toBeUndefined();
  });
});
