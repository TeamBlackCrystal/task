import { describe, it, expect, vi, beforeEach } from 'vitest';
import { defineComponent, ref } from 'vue';
import { mount, flushPromises } from '@vue/test-utils';
import { VueQueryPlugin, QueryClient } from '@tanstack/vue-query';
import type { paths } from '@/generated/api';
import { useTaskDetail } from '../useTaskDetail';

const GET_TASK_PATH = '/v1/tenants/{tenant_id}/projects/{project_id}/tasks/{id}' as const;
const LIST_TASKS_PATH = '/v1/tenants/{tenant_id}/projects/{project_id}/tasks' as const;
const TASK_SEARCH_PATH = '/v1/tenants/{tenant_id}/projects/{project_id}/tasks/search' as const;

// vi.mock の factory から参照するため hoisted に置く
const {
  TENANT_ID,
  PROJECT_ID,
  TASK_SEQ_KEY,
  OTHER_TASK_SEQ_KEY,
  baseTask,
  otherTask,
  putControl,
  labelsControl,
  membersControl,
  assigneeControl,
  fetchMock,
} = vi.hoisted(() => {
  const TENANT_ID = 'tenant-1';
  const PROJECT_ID = 'project-1';
  const TASK_SEQ_KEY = 'ENG-1';
  const OTHER_TASK_SEQ_KEY = 'ENG-2';

  const baseTask = {
    id: '00000000-0000-0000-0000-000000000010',
    seq_key: TASK_SEQ_KEY,
    title: '元のタイトル',
    description: null,
    status_id: 'status-1',
    priority: 'Medium',
    progress_pct: 0,
    soft_deadline: null,
    hard_deadline: null,
    // テストごとに差し替えるため、空配列（never[]）に推論させない
    labels: [] as Record<string, unknown>[],
    assignees: [] as Record<string, unknown>[],
  };

  const otherTask = {
    ...baseTask,
    id: '00000000-0000-0000-0000-000000000011',
    seq_key: OTHER_TASK_SEQ_KEY,
    title: '別のタスク',
    assignees: [] as Record<string, unknown>[],
  };

  function jsonResponse(body: unknown, status = 200) {
    return new Response(JSON.stringify(body), {
      status,
      headers: { 'Content-Type': 'application/json' },
    });
  }

  // PUT を保留して任意のタイミングで完了（成功/失敗）させるための deferred
  const putControl: {
    resolve?: (task: Record<string, unknown>) => void;
    fail?: (status?: number) => void;
  } = {};

  // ラベル一覧 GET の応答をテストごとに切り替える
  const labelsControl: { mode: 'success' | 'error' | 'pending'; data: unknown[] } = {
    mode: 'success',
    data: [],
  };

  const membersControl: { data: unknown[] } = { data: [] };

  // 担当者の POST を保留して任意のタイミングで完了させるための deferred
  const assigneeControl: { resolve?: () => void } = {};

  const fetchMock = async (input: Request) => {
    const url = input.url;
    const method = input.method.toUpperCase();

    if (method === 'GET' && url.endsWith(`/tasks/${TASK_SEQ_KEY}`)) {
      return jsonResponse(baseTask);
    }
    if (method === 'GET' && url.endsWith(`/tasks/${OTHER_TASK_SEQ_KEY}`)) {
      return jsonResponse(otherTask);
    }
    if (method === 'GET' && url.endsWith('/members')) {
      return jsonResponse(membersControl.data);
    }
    if (method === 'POST' && url.includes('/assignees')) {
      return new Promise<Response>((resolve) => {
        assigneeControl.resolve = () => resolve(new Response(null, { status: 201 }));
      });
    }
    if (method === 'GET' && url.endsWith('/statuses')) {
      return jsonResponse([]);
    }
    if (method === 'GET' && url.endsWith('/labels')) {
      if (labelsControl.mode === 'pending') return new Promise<Response>(() => {});
      if (labelsControl.mode === 'error') return jsonResponse({ message: 'boom' }, 500);
      return jsonResponse(labelsControl.data);
    }
    if (method === 'PUT' && url.endsWith(`/tasks/${TASK_SEQ_KEY}`)) {
      return new Promise<Response>((resolve) => {
        putControl.resolve = (task) => resolve(jsonResponse(task));
        putControl.fail = (status = 400) => resolve(jsonResponse({ message: 'invalid' }, status));
      });
    }
    if (method === 'DELETE' && url.endsWith(`/tasks/${TASK_SEQ_KEY}`)) {
      return new Response(null, { status: 204 });
    }
    return jsonResponse({ message: 'not found' }, 404);
  };

  return {
    TENANT_ID,
    PROJECT_ID,
    TASK_SEQ_KEY,
    OTHER_TASK_SEQ_KEY,
    baseTask,
    otherTask,
    putControl,
    labelsControl,
    membersControl,
    assigneeControl,
    fetchMock,
  };
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

vi.mock('@/composables/useResolvedTenantId', () => ({
  useResolvedTenantId: () => ({
    tenantId: ref(TENANT_ID),
    isTenantNotFound: ref(false),
    isResolving: ref(false),
    isError: ref(false),
  }),
}));

vi.mock('@/composables/useResolvedProjectId', () => ({
  useResolvedProjectId: () => ({
    projectId: ref(PROJECT_ID),
    isProjectNotFound: ref(false),
    isResolving: ref(false),
    isError: ref(false),
  }),
}));

const taskQueryKey = [
  'get',
  GET_TASK_PATH,
  { params: { path: { tenant_id: TENANT_ID, project_id: PROJECT_ID, id: TASK_SEQ_KEY } } },
] as const;
const listQueryKey = [
  'get',
  LIST_TASKS_PATH,
  { params: { path: { tenant_id: TENANT_ID, project_id: PROJECT_ID } } },
] as const;
const searchQueryKey = [
  'get',
  TASK_SEARCH_PATH,
  {
    params: {
      path: { tenant_id: TENANT_ID, project_id: PROJECT_ID },
      query: { q: '元の' },
    },
  },
] as const;

describe('useTaskDetail のキャッシュ同期', () => {
  let queryClient: QueryClient;
  let detail: ReturnType<typeof useTaskDetail>;
  let onAfterDelete: ReturnType<typeof vi.fn<(listHref: string) => void>>;

  function mountHost() {
    const Host = defineComponent({
      setup() {
        detail = useTaskDetail({
          tenantDisplayId: 'acme',
          projectKey: 'ENG',
          taskId: TASK_SEQ_KEY,
          onAfterDelete,
        });
        return () => null;
      },
    });
    return mount(Host, {
      global: { plugins: [[VueQueryPlugin, { queryClient }]] },
    });
  }

  function seedListAndSearchCaches() {
    queryClient.setQueryData(listQueryKey, { tasks: [baseTask] });
    queryClient.setQueryData(searchQueryKey, { tasks: [baseTask] });
  }

  beforeEach(() => {
    queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    onAfterDelete = vi.fn<(listHref: string) => void>();
    putControl.resolve = undefined;
    labelsControl.mode = 'success';
    labelsControl.data = [];
    baseTask.labels = [];
  });

  /**
   * 担当者の更新は専用の POST / DELETE を使うため、共通の mutateTask を通らない。
   * 応答を待つ間に別タスクへ移ると、リアクティブな query key を使っていると
   * 更新したタスクではなく移動先を invalidate してしまう。
   */
  it('応答前に別タスクへ移っても、更新したタスクのキャッシュを invalidate する', async () => {
    membersControl.data = [
      {
        id: 'member-1',
        project_id: PROJECT_ID,
        role: 'Member',
        user_id: 'user-1',
        user: { id: 'user-1', username: 'yupix', avatar_url: null },
      },
    ];
    const currentTaskId = ref(TASK_SEQ_KEY);
    const Host = defineComponent({
      setup() {
        detail = useTaskDetail({
          tenantDisplayId: 'acme',
          projectKey: 'ENG',
          taskId: currentTaskId,
          onAfterDelete,
        });
        return () => null;
      },
    });
    mount(Host, { global: { plugins: [[VueQueryPlugin, { queryClient }]] } });
    await flushPromises();

    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    detail.onToggleAssignee('user-1');
    await flushPromises();

    // 応答を待たずに別タスクへ移る（分割ビューでの選択替え）
    currentTaskId.value = OTHER_TASK_SEQ_KEY;
    await flushPromises();

    assigneeControl.resolve?.();
    await flushPromises();

    const detailInvalidations = invalidateSpy.mock.calls
      .map(([arg]) => (arg as { queryKey?: readonly unknown[] } | undefined)?.queryKey)
      .filter((key): key is readonly unknown[] => key?.[1] === GET_TASK_PATH);

    expect(detailInvalidations).not.toHaveLength(0);
    for (const key of detailInvalidations) {
      expect(key).toEqual(taskQueryKey);
    }
  });

  it('ラベル一覧の取得失敗は projectLabelsError として公開し、詳細全体の isError にはしない', async () => {
    labelsControl.mode = 'error';
    mountHost();
    await flushPromises();

    expect(detail.projectLabelsError.value).toBe(true);
    expect(detail.projectLabelsLoading.value).toBe(false);
    expect(detail.projectLabels.value).toEqual([]);
    expect(detail.isError.value).toBe(false);
    expect(detail.isLoading.value).toBe(false);
  });

  it('ラベル一覧の取得中は projectLabelsLoading が true になる', async () => {
    labelsControl.mode = 'pending';
    mountHost();
    await flushPromises();

    expect(detail.projectLabelsLoading.value).toBe(true);
    expect(detail.projectLabelsError.value).toBe(false);
    expect(detail.isLoading.value).toBe(false);
  });

  it('ペイン切替（unmount）後に完了した更新でも、詳細キャッシュ更新と一覧・検索の invalidate が走る', async () => {
    const wrapper = mountHost();
    await flushPromises();
    expect(queryClient.getQueryData(taskQueryKey)).toMatchObject({ title: '元のタイトル' });

    seedListAndSearchCaches();

    detail.onSaveTitle('新しいタイトル');
    await flushPromises();
    expect(putControl.resolve).toBeDefined();

    // PUT が完了する前にペインを切り替える（コンポーネント再生成で unmount）
    wrapper.unmount();

    putControl.resolve!({ ...baseTask, title: '新しいタイトル' });
    await flushPromises();

    expect(queryClient.getQueryData(taskQueryKey)).toMatchObject({ title: '新しいタイトル' });
    expect(queryClient.getQueryState(listQueryKey)?.isInvalidated).toBe(true);
    expect(queryClient.getQueryState(searchQueryKey)?.isInvalidated).toBe(true);
  });

  it('検索結果表示中の更新で検索キャッシュも invalidate される（mount したまま）', async () => {
    mountHost();
    await flushPromises();
    seedListAndSearchCaches();

    detail.onSaveTitle('新しいタイトル');
    await flushPromises();
    putControl.resolve!({ ...baseTask, title: '新しいタイトル' });
    await flushPromises();

    expect(queryClient.getQueryState(searchQueryKey)?.isInvalidated).toBe(true);
    expect(queryClient.getQueryState(listQueryKey)?.isInvalidated).toBe(true);
  });

  it('ラベル更新の楽観値を名前順、同名時は ID 順に並べる', async () => {
    const alpha1 = {
      id: '00000000-0000-0000-0000-000000000001',
      name: 'alpha',
      description: '',
      color: '#111111',
      icon_url: null,
      project_id: PROJECT_ID,
    };
    const alpha2 = { ...alpha1, id: '00000000-0000-0000-0000-000000000002' };
    const beta = {
      ...alpha1,
      id: '00000000-0000-0000-0000-000000000003',
      name: 'beta',
    };
    labelsControl.data = [beta, alpha2, alpha1];
    mountHost();
    await vi.waitFor(() => {
      expect(detail.projectLabels.value).toEqual([beta, alpha2, alpha1]);
    });

    detail.onSaveLabels([beta.id, alpha2.id, alpha1.id]);
    await vi.waitFor(() => {
      expect(detail.displayTask.value?.labels).toEqual([alpha1, alpha2, beta]);
    });

    putControl.resolve!({ ...baseTask, labels: [alpha1, alpha2, beta] });
    await flushPromises();
  });

  it('ラベル一覧が古くて欠けていても、既にタスクに付いているラベルは楽観値に残す', async () => {
    const kept = {
      id: '00000000-0000-0000-0000-000000000001',
      name: 'kept',
      description: '',
      color: '#111111',
      icon_url: null,
      project_id: PROJECT_ID,
    };
    const added = { ...kept, id: '00000000-0000-0000-0000-000000000002', name: 'added' };

    // タスクには kept が付いているが、独立キャッシュのラベル一覧は古くて kept を含まない。
    // 楽観値を一覧だけから作ると、kept が無関係に一瞬消えて見える
    baseTask.labels = [kept];
    labelsControl.data = [added];
    mountHost();
    await vi.waitFor(() => {
      expect(detail.projectLabels.value).toEqual([added]);
    });

    detail.onSaveLabels([kept.id, added.id]);
    await vi.waitFor(() => {
      expect(detail.displayTask.value?.labels).toEqual([added, kept]);
    });

    putControl.resolve!({ ...baseTask, labels: [added, kept] });
    await flushPromises();
  });

  it('ラベル名の並びはコードポイント順（サーバの Rust 側と一致し、UTF-16 順にしない）', async () => {
    const base = {
      description: '',
      color: '#111111',
      icon_url: null,
      project_id: PROJECT_ID,
    };
    // U+1F41B（補助面、UTF-16 ではサロゲートペア 0xD83D…）と U+FF8A（BMP 後方）。
    // UTF-16 コードユニット順だと 🐛bug が先、コードポイント順だと ﾊﾞｸﾞ が先になる
    const emoji = { ...base, id: '00000000-0000-0000-0000-000000000001', name: '🐛bug' };
    const halfKana = { ...base, id: '00000000-0000-0000-0000-000000000002', name: 'ﾊﾞｸﾞ' };
    labelsControl.data = [emoji, halfKana];
    mountHost();
    await vi.waitFor(() => {
      expect(detail.projectLabels.value).toEqual([emoji, halfKana]);
    });

    detail.onSaveLabels([emoji.id, halfKana.id]);
    await vi.waitFor(() => {
      expect(detail.displayTask.value?.labels).toEqual([halfKana, emoji]);
    });

    putControl.resolve!({ ...baseTask, labels: [halfKana, emoji] });
    await flushPromises();
  });

  it('ラベル保存が 400 で失敗したら楽観値を巻き戻し、タスクとラベル一覧を再取得する', async () => {
    const bug = {
      id: '00000000-0000-0000-0000-000000000001',
      name: 'bug',
      description: '',
      color: '#111111',
      icon_url: null,
      project_id: PROJECT_ID,
    };
    labelsControl.data = [bug];
    mountHost();
    await flushPromises();
    const labelsQueryKey = [
      'get',
      '/v1/tenants/{tenant_id}/projects/{project_id}/labels',
      { params: { path: { tenant_id: TENANT_ID, project_id: PROJECT_ID } } },
    ] as const;
    expect(queryClient.getQueryState(labelsQueryKey)).toBeDefined();

    detail.onSaveLabels([bug.id]);
    await flushPromises();
    expect(detail.displayTask.value?.labels).toEqual([bug]);
    const taskUpdates = queryClient.getQueryState(taskQueryKey)!.dataUpdateCount;
    const labelsUpdates = queryClient.getQueryState(labelsQueryKey)!.dataUpdateCount;

    putControl.fail!(400);
    await flushPromises();

    expect(detail.displayTask.value?.labels).toEqual([]);
    expect(detail.labelsError.value).toBe('ラベルの更新に失敗しました');
    // invalidate による再取得が完走していること（observer があるので即 refetch される）
    expect(queryClient.getQueryState(taskQueryKey)!.dataUpdateCount).toBeGreaterThan(taskUpdates);
    expect(queryClient.getQueryState(labelsQueryKey)!.dataUpdateCount).toBeGreaterThan(
      labelsUpdates,
    );
  });

  it('優先度の更新が 400 で失敗したら楽観値を巻き戻し、エラーを立てる', async () => {
    mountHost();
    await flushPromises();

    detail.onPriorityChange('High');
    await flushPromises();
    // 飛行中は楽観値が出ていて、二度目の選択を止めるフラグが立っている
    expect(detail.displayTask.value?.priority).toBe('High');
    expect(detail.priorityUpdating.value).toBe(true);

    putControl.fail!(400);
    await flushPromises();

    // 巻き戻さないと、失敗したのに変わったように見えたまま残る
    expect(detail.displayTask.value?.priority).toBe('Medium');
    expect(detail.priorityError.value).toBe('優先度の更新に失敗しました');
    expect(detail.priorityUpdating.value).toBe(false);
  });

  it('削除成功時に検索キャッシュが invalidate され、詳細キャッシュは除去される', async () => {
    mountHost();
    await flushPromises();
    seedListAndSearchCaches();

    detail.confirmDelete();
    await flushPromises();

    expect(queryClient.getQueryState(taskQueryKey)).toBeUndefined();
    expect(queryClient.getQueryState(listQueryKey)?.isInvalidated).toBe(true);
    expect(queryClient.getQueryState(searchQueryKey)?.isInvalidated).toBe(true);
    expect(onAfterDelete).toHaveBeenCalledWith('/acme/projects/ENG/tasks');
  });
});
