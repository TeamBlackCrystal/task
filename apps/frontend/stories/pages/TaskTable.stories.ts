import type { Meta, StoryObj } from '@storybook/vue3-vite';
import { expect, fn, screen, userEvent, within } from 'storybook/test';
import { provide, reactive, nextTick } from 'vue';
import { QueryClient, VUE_QUERY_CLIENT } from '@tanstack/vue-query';
import TaskTablePage from '@/pages/@tenant/projects/@projectKey/tasks/+Page.vue';

const PAGE_CONTEXT_KEY = 'vike-vue:usePageContext';

const mockContext = {
  urlPathname: '/tenant-123/projects/ENG/tasks',
  routeParams: { tenant: 'tenant-123', projectKey: 'ENG' },
};

const TENANT_UUID = '11111111-1111-1111-1111-111111111111';

const sampleTenants = (displayId: string) => [
  {
    id: TENANT_UUID,
    display_id: displayId,
    name: 'テストテナント',
    description: '',
    icon_url: '',
    owner_id: '00000000-0000-0000-0000-000000000002',
    require_2fa: false,
  },
];

const isListTenantsUrl = (url: string) => {
  try {
    const pathname = new URL(url, 'http://localhost').pathname;
    return /\/v1\/tenants\/?$/.test(pathname);
  } catch {
    return /\/v1\/tenants\/?(?:\?|$)/.test(url) && !/\/v1\/tenants\/[^/?]/.test(url);
  }
};

const jsonResponse = (data: unknown, status = 200) =>
  new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });

const sampleProjects = [
  {
    id: 'proj-eng',
    key: 'ENG',
    name: 'エンジニアリング',
    description: '',
    tenant_id: 'tenant-123',
    is_personal: false,
  },
  {
    id: 'proj-mkt',
    key: 'MKT',
    name: 'マーケティング',
    description: '',
    tenant_id: 'tenant-123',
    is_personal: false,
  },
];

const sampleStatuses = [
  {
    id: 's-backlog',
    name: 'Backlog',
    color: '#94a3b8',
    position: 0,
    is_default: true,
    is_done_state: false,
    project_id: 'proj-eng',
    created_at: '2026-01-01T00:00:00Z',
  },
  {
    id: 's-progress',
    name: 'In Progress',
    color: '#3b82f6',
    position: 1,
    is_default: false,
    is_done_state: false,
    project_id: 'proj-eng',
    created_at: '2026-01-01T00:00:00Z',
  },
  {
    id: 's-review',
    name: 'In Review',
    color: '#8b5cf6',
    position: 2,
    is_default: false,
    is_done_state: false,
    project_id: 'proj-eng',
    created_at: '2026-01-01T00:00:00Z',
  },
  {
    id: 's-done',
    name: 'Done',
    color: '#22c55e',
    position: 3,
    is_default: false,
    is_done_state: true,
    project_id: 'proj-eng',
    created_at: '2026-01-01T00:00:00Z',
  },
];

/** Chromatic 日次差分揺れ防止の固定ユーザー */
const mockUsers = {
  alpha: {
    id: '11111111-1111-4111-8111-111111111101',
    username: '田中太郎',
    avatar_url: null,
  },
  beta: {
    id: '11111111-1111-4111-8111-111111111102',
    username: '佐藤花子',
    avatar_url: null,
  },
  gamma: {
    id: '11111111-1111-4111-8111-111111111103',
    username: '鈴木一郎',
    avatar_url: null,
  },
  delta: {
    id: '11111111-1111-4111-8111-111111111104',
    username: '高橋美咲',
    avatar_url: null,
  },
  epsilon: {
    id: '11111111-1111-4111-8111-111111111105',
    username: '伊藤健太',
    avatar_url: null,
  },
} as const;

/** 担当者の候補（作成ダイアログが引くプロジェクトメンバー） */
const sampleProjectMembers = Object.values(mockUsers).map((user, index) => ({
  id: `member-${index + 1}`,
  project_id: 'proj-eng',
  user_id: user.id,
  role: index === 0 ? 'admin' : 'member',
  user,
}));

const assignee = (user: (typeof mockUsers)[keyof typeof mockUsers]) => ({
  role: 'assignee',
  user,
});

const sampleTasks = {
  tasks: [
    {
      id: 'task-1',
      seq_id: 1,
      title: 'OAuth 対応を実装する',
      priority: 'High' as const,
      status_id: 's-progress',
      project_id: 'proj-eng',
      soft_deadline: '2026-07-02T00:00:00Z',
      hard_deadline: null,
      is_archived: false,
      labels: [
        {
          id: 'label-bug',
          name: 'bug',
          description: '',
          color: '#e11d48',
          icon_url: null,
          project_id: 'proj-eng',
        },
      ],
      progress_pct: 0,
      created_at: '2026-06-01T00:00:00Z',
      updated_at: '2026-06-15T00:00:00Z',
      created_by: mockUsers.alpha,
      assignees: [assignee(mockUsers.alpha), assignee(mockUsers.beta), assignee(mockUsers.gamma)],
    },
    {
      id: 'task-2',
      seq_id: 2,
      title: 'ログイン画面の UI 実装',
      priority: 'Medium' as const,
      status_id: 's-review',
      project_id: 'proj-eng',
      soft_deadline: '2026-06-29T00:00:00Z',
      hard_deadline: null,
      is_archived: false,
      labels: [],
      progress_pct: 0,
      created_at: '2026-06-01T00:00:00Z',
      updated_at: '2026-06-15T00:00:00Z',
      created_by: mockUsers.alpha,
      assignees: [assignee(mockUsers.beta)],
    },
    {
      id: 'task-3',
      seq_id: 3,
      title: 'DB スキーマ設計',
      priority: 'Critical' as const,
      status_id: 's-done',
      project_id: 'proj-eng',
      soft_deadline: null,
      hard_deadline: null,
      is_archived: false,
      labels: [],
      progress_pct: 100,
      created_at: '2026-06-01T00:00:00Z',
      updated_at: '2026-06-15T00:00:00Z',
      created_by: mockUsers.alpha,
      assignees: [assignee(mockUsers.gamma)],
    },
    {
      id: 'task-4',
      seq_id: 4,
      title: '通知メール送信機能',
      priority: 'Low' as const,
      status_id: 's-progress',
      project_id: 'proj-eng',
      soft_deadline: '2026-07-14T00:00:00Z',
      hard_deadline: null,
      is_archived: false,
      labels: [],
      progress_pct: 0,
      created_at: '2026-06-01T00:00:00Z',
      updated_at: '2026-06-15T00:00:00Z',
      created_by: mockUsers.alpha,
      assignees: [],
    },
    {
      id: 'task-5',
      seq_id: 5,
      title: 'タスク一覧 API の実装',
      priority: 'Medium' as const,
      status_id: 's-backlog',
      project_id: 'proj-eng',
      soft_deadline: null,
      hard_deadline: null,
      is_archived: false,
      labels: [],
      progress_pct: 0,
      created_at: '2026-06-01T00:00:00Z',
      updated_at: '2026-06-15T00:00:00Z',
      created_by: mockUsers.alpha,
      assignees: [assignee(mockUsers.alpha)],
    },
    {
      id: 'task-6',
      seq_id: 6,
      title: 'チーム全体ミーティング調整',
      priority: 'Low' as const,
      status_id: 's-backlog',
      project_id: 'proj-eng',
      soft_deadline: null,
      hard_deadline: null,
      is_archived: false,
      labels: [],
      progress_pct: 0,
      created_at: '2026-06-01T00:00:00Z',
      updated_at: '2026-06-15T00:00:00Z',
      created_by: mockUsers.alpha,
      assignees: [
        assignee(mockUsers.alpha),
        assignee(mockUsers.beta),
        assignee(mockUsers.gamma),
        assignee(mockUsers.delta),
        assignee(mockUsers.epsilon),
      ],
    },
  ],
  total: 6,
};

const sampleLabels = [
  {
    id: 'label-bug',
    name: 'bug',
    description: '',
    color: '#e11d48',
    icon_url: null,
    project_id: 'proj-eng',
  },
  {
    id: 'label-feature',
    name: 'feature',
    description: '',
    color: '#3b82f6',
    icon_url: null,
    project_id: 'proj-eng',
  },
];

const sampleSearchTasks = {
  tasks: [
    {
      id: 'task-search-1',
      seq_id: 42,
      title: 'OAuth 検索結果',
      highlight: '<em>OAuth</em> 認証 &amp; フローを更新する',
      score: 0.98,
    },
  ],
  total: 75,
};

/**
 * fetch モックで全 API エンドポイントを差し替える
 */
function createMockFetch(
  overrides: {
    projects?: typeof sampleProjects;
    statuses?: typeof sampleStatuses;
    tasks?: { tasks: unknown[]; total: number };
    searchTasks?: typeof sampleSearchTasks;
    rejectSearch?: boolean;
    rejectAll?: boolean;
    rejectTenantsList?: boolean;
    rejectLabels?: boolean;
    hang?: boolean;
  } = {},
) {
  const original = globalThis.fetch;
  globalThis.fetch = fn().mockImplementation(async (req: Request) => {
    const url = typeof req === 'string' ? req : req.url;
    if (isListTenantsUrl(url)) {
      if (overrides.rejectTenantsList) {
        return jsonResponse({ message: 'server error' }, 500);
      }
      return jsonResponse(sampleTenants(mockContext.routeParams.tenant));
    }
    if (overrides.rejectAll) {
      throw new TypeError('Failed to fetch');
    }
    if (overrides.hang) {
      return new Promise(() => {});
    }
    if (url.includes('/statuses')) {
      return jsonResponse(overrides.statuses ?? sampleStatuses);
    }
    if (url.includes('/labels')) {
      if (overrides.rejectLabels) {
        return jsonResponse({ message: 'server error' }, 500);
      }
      return jsonResponse(sampleLabels);
    }
    if (url.includes('/tasks/search')) {
      if (overrides.rejectSearch) {
        return jsonResponse({ message: 'search failed' }, 500);
      }
      return jsonResponse(overrides.searchTasks ?? sampleSearchTasks);
    }
    // 単体タスク詳細（分割ビューのペインが叩く）: /tasks/{seqKey}
    const detailMatch = url.match(/\/tasks\/([^/?]+)(?:\?|$)/);
    if (detailMatch) {
      const list = (overrides.tasks ?? sampleTasks).tasks as Array<{ seq_id: number }>;
      const found = list.find(
        (t) => `${mockContext.routeParams.projectKey}-${t.seq_id}` === detailMatch[1],
      );
      // 詳細レスポンスは labels を含む（一覧用フィクスチャには無いのでここで補う）
      return jsonResponse({ ...(found ?? list[0]), labels: [] });
    }
    if (url.includes('/members')) {
      return jsonResponse(sampleProjectMembers);
    }
    if (url.includes('/tasks')) {
      return jsonResponse(overrides.tasks ?? sampleTasks);
    }
    // 残ったプロジェクト系だけを最後に受ける
    if (url.includes('/v1/tenants/') && url.includes('/projects')) {
      return jsonResponse(overrides.projects ?? sampleProjects);
    }
    return jsonResponse({});
  });
  return () => {
    globalThis.fetch = original;
  };
}

function mockFetch() {
  return createMockFetch();
}

const mktStatuses = [
  {
    id: 'm-backlog',
    name: 'Backlog',
    color: '#94a3b8',
    position: 0,
    is_default: true,
    is_done_state: false,
    project_id: 'proj-mkt',
    created_at: '2026-01-01T00:00:00Z',
  },
];

const mktLabels = [
  {
    id: 'label-campaign',
    name: 'campaign',
    description: '',
    color: '#8b5cf6',
    icon_url: null,
    project_id: 'proj-mkt',
  },
];

const mktSampleTasks = {
  tasks: [
    {
      id: 'mkt-task-1',
      seq_id: 1,
      title: 'SNSキャンペーン企画',
      priority: 'High' as const,
      status_id: 'm-backlog',
      project_id: 'proj-mkt',
      soft_deadline: null,
      hard_deadline: null,
      is_archived: false,
      labels: [],
      progress_pct: 0,
      created_at: '2026-06-01T00:00:00Z',
      updated_at: '2026-06-15T00:00:00Z',
      created_by: mockUsers.alpha,
      assignees: [assignee(mockUsers.beta)],
    },
  ],
  total: 1,
};

type ProjectSwitchMock = {
  restore: () => void;
  releaseMktTasks: () => void;
};

function createProjectSwitchMockFetch(): ProjectSwitchMock {
  const original = globalThis.fetch;
  let releaseMktTasks: (() => void) | null = null;
  const mktTasksGate = new Promise<void>((resolve) => {
    releaseMktTasks = resolve;
  });

  globalThis.fetch = fn().mockImplementation(async (req: Request) => {
    const url = typeof req === 'string' ? req : req.url;
    if (isListTenantsUrl(url)) {
      return jsonResponse(sampleTenants(mockContext.routeParams.tenant));
    }
    if (url.includes('/labels') && url.includes('proj-mkt')) {
      return jsonResponse(mktLabels);
    }
    if (url.includes('/labels')) {
      return jsonResponse(sampleLabels);
    }
    if (url.includes('/statuses') && url.includes('proj-mkt')) {
      return jsonResponse(mktStatuses);
    }
    if (url.includes('/statuses')) {
      return jsonResponse(sampleStatuses);
    }
    if (url.includes('/members')) {
      return jsonResponse(sampleProjectMembers);
    }
    if (url.includes('/tasks') && url.includes('proj-mkt')) {
      await mktTasksGate;
      return jsonResponse(mktSampleTasks);
    }
    if (url.includes('/tasks')) {
      return jsonResponse(sampleTasks);
    }
    // 残ったプロジェクト系だけを最後に受ける
    if (url.includes('/v1/tenants/') && url.includes('/projects')) {
      return jsonResponse(sampleProjects);
    }
    return jsonResponse({});
  });

  return {
    restore: () => {
      globalThis.fetch = original;
    },
    releaseMktTasks: () => releaseMktTasks?.(),
  };
}

/** ラベルフィルタ適用時（label_id=label-bug）のみ返す絞り込み結果 */
const bugFilteredTasks = {
  tasks: [sampleTasks.tasks[0]],
  total: 1,
};

type LabelFilterMock = {
  restore: () => void;
  releaseFilteredTasks: () => void;
};

/** ラベル絞り込みリクエストだけを保留し、旧条件データの残留を検証できるモック */
function createLabelFilterMockFetch(): LabelFilterMock {
  const original = globalThis.fetch;
  let releaseFilteredTasks: (() => void) | null = null;
  const filteredTasksGate = new Promise<void>((resolve) => {
    releaseFilteredTasks = resolve;
  });

  globalThis.fetch = fn().mockImplementation(async (req: Request) => {
    const url = typeof req === 'string' ? req : req.url;
    if (isListTenantsUrl(url)) {
      return jsonResponse(sampleTenants(mockContext.routeParams.tenant));
    }
    if (url.includes('/statuses')) {
      return jsonResponse(sampleStatuses);
    }
    if (url.includes('/labels')) {
      return jsonResponse(sampleLabels);
    }
    if (url.includes('/members')) {
      return jsonResponse(sampleProjectMembers);
    }
    if (url.includes('/tasks') && url.includes('label_id=label-bug')) {
      await filteredTasksGate;
      return jsonResponse(bugFilteredTasks);
    }
    if (url.includes('/tasks')) {
      return jsonResponse(sampleTasks);
    }
    // 残ったプロジェクト系だけを最後に受ける
    if (url.includes('/v1/tenants/') && url.includes('/projects')) {
      return jsonResponse(sampleProjects);
    }
    return jsonResponse({});
  });

  return {
    restore: () => {
      globalThis.fetch = original;
    },
    releaseFilteredTasks: () => releaseFilteredTasks?.(),
  };
}

let reactivePageContext: ReturnType<typeof reactive<typeof mockContext>> | null = null;
let projectSwitchMock: ProjectSwitchMock | null = null;
let labelFilterMock: LabelFilterMock | null = null;

function storyDecoratorReactive() {
  return () => ({
    setup() {
      const queryClient = new QueryClient({
        defaultOptions: {
          queries: { retry: false, gcTime: 0, staleTime: 0 },
          mutations: { retry: false },
        },
      });
      provide(VUE_QUERY_CLIENT, queryClient);
      reactivePageContext = reactive({
        ...mockContext,
        routeParams: { ...mockContext.routeParams },
      });
      provide(PAGE_CONTEXT_KEY, reactivePageContext);
    },
    template: '<story />',
  });
}

function storyDecorator(
  context: { urlPathname: string; routeParams: Record<string, string> } = mockContext,
) {
  return () => ({
    setup() {
      const queryClient = new QueryClient({
        defaultOptions: {
          queries: { retry: false, gcTime: 0, staleTime: 0 },
          mutations: { retry: false },
        },
      });
      provide(VUE_QUERY_CLIENT, queryClient);
      provide(PAGE_CONTEXT_KEY, context);
    },
    template: '<story />',
  });
}

const meta = {
  title: 'Pages/TaskTable',
  component: TaskTablePage,
  tags: ['autodocs'],
  parameters: {
    layout: 'padded',
    docs: {
      description: {
        component:
          'プロジェクトタスク一覧の TanStack Table ビュー。fetch モックで全 API エンドポイントを差し替え。' +
          ' 入力のデバウンス後にプロジェクトスコープのサーバー側検索 API へ接続する。',
      },
    },
  },
  decorators: [storyDecorator()],
} satisfies Meta<typeof TaskTablePage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  name: 'タスクあり',
  beforeEach: mockFetch,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    // 既存タスク
    await expect(canvas.findByText('OAuth 対応を実装する')).resolves.toBeInTheDocument();
    await expect(canvas.findByText('ログイン画面の UI 実装')).resolves.toBeInTheDocument();
    await expect(canvas.findByText('ENG-1')).resolves.toBeInTheDocument();
    // 新規タスク
    await expect(canvas.findByText('チーム全体ミーティング調整')).resolves.toBeInTheDocument();
    // 複数担当者オーバーフロー表示（task-6 は5名, maxDisplay=3 → overflowCount=2 → 他2名）
    await expect(canvas.findByText(/他2名/)).resolves.toBeInTheDocument();
  },
};

export const Empty: Story = {
  name: 'タスクなし',
  beforeEach: () => createMockFetch({ tasks: { tasks: [], total: 0 } }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.findByText('タスクが見つかりません')).resolves.toBeInTheDocument();
  },
};

export const SearchResults: Story = {
  name: 'サーバー検索・正常',
  beforeEach: mockFetch,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const searchInput = await canvas.findByRole('searchbox', { name: 'タスクを検索' });
    await userEvent.type(searchInput, 'OAuth');

    await expect(canvas.findByText('OAuth 検索結果')).resolves.toBeInTheDocument();
    const highlight = canvasElement.querySelector('td em');
    await expect(highlight).toBeInTheDocument();
    await expect(highlight).toHaveTextContent('OAuth');
    await expect(highlight?.closest('td')).toHaveTextContent('OAuth 認証 & フローを更新する');
    await expect(
      canvas.findByText(/上位\s+1\s+件\s+\/\s+全\s+75\s+件/),
    ).resolves.toBeInTheDocument();
    await expect(canvas.findByText('ENG-42')).resolves.toBeInTheDocument();
    await expect(canvas.queryByText('ログイン画面の UI 実装')).not.toBeInTheDocument();
  },
};

export const EmptySearchInput: Story = {
  name: 'サーバー検索・空入力',
  beforeEach: mockFetch,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const searchInput = await canvas.findByRole('searchbox', { name: 'タスクを検索' });
    await userEvent.type(searchInput, '   ');
    await new Promise((resolve) => setTimeout(resolve, 400));

    await expect(canvas.findByText('OAuth 対応を実装する')).resolves.toBeInTheDocument();
    await expect(canvas.queryByText('OAuth 検索結果')).not.toBeInTheDocument();
  },
};

export const SearchNoResults: Story = {
  name: 'サーバー検索・0件',
  beforeEach: () => createMockFetch({ searchTasks: { tasks: [], total: 0 } }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const searchInput = await canvas.findByRole('searchbox', { name: 'タスクを検索' });
    await userEvent.type(searchInput, '存在しないタスク');

    await expect(canvas.findByText('検索結果がありません')).resolves.toBeInTheDocument();
    await expect(
      canvas.findByText(/上位\s+0\s+件\s+\/\s+全\s+0\s+件/),
    ).resolves.toBeInTheDocument();
  },
};

export const SearchApiError: Story = {
  name: 'サーバー検索・APIエラー',
  beforeEach: () => createMockFetch({ rejectSearch: true }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const searchInput = await canvas.findByRole('searchbox', { name: 'タスクを検索' });
    await userEvent.type(searchInput, '失敗する検索');

    await expect(canvas.findByText('検索に失敗しました')).resolves.toBeInTheDocument();
    await expect(canvas.findByRole('button', { name: '再試行' })).resolves.toBeInTheDocument();
  },
};

export const ProjectNotFound: Story = {
  name: 'プロジェクトなし',
  decorators: [
    storyDecorator({
      urlPathname: '/tenant-123/projects/UNKNOWN/tasks',
      routeParams: { tenant: 'tenant-123', projectKey: 'UNKNOWN' },
    }),
  ],
  beforeEach: mockFetch,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.findByText('プロジェクトが見つかりません')).resolves.toBeInTheDocument();
  },
};

export const ApiError: Story = {
  name: 'API エラー',
  beforeEach: () => createMockFetch({ rejectAll: true }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.findByText('タスクの読み込みに失敗しました')).resolves.toBeInTheDocument();
  },
};

export const TenantResolveError: Story = {
  name: 'テナント解決エラー',
  beforeEach: () => createMockFetch({ rejectTenantsList: true }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.findByText('タスクの読み込みに失敗しました')).resolves.toBeInTheDocument();
    await expect(canvas.queryByText('タスクが見つかりません')).not.toBeInTheDocument();
  },
};

export const Loading: Story = {
  name: 'ロード中',
  beforeEach: () => createMockFetch({ hang: true }),
};

export const Sorting: Story = {
  name: 'ソート操作',
  beforeEach: mockFetch,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const user = userEvent.setup();
    const titleHeader = await canvas.findByRole('button', { name: /タイトル/ });
    await user.click(titleHeader);
    await expect(canvas.findByText('OAuth 対応を実装する')).resolves.toBeInTheDocument();
  },
};

export const ProjectSwitch: Story = {
  name: 'プロジェクト切替で旧タスク非表示',
  decorators: [storyDecoratorReactive()],
  beforeEach() {
    projectSwitchMock = createProjectSwitchMockFetch();
    return () => {
      projectSwitchMock?.restore();
      projectSwitchMock = null;
      reactivePageContext = null;
    };
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    if (!projectSwitchMock) {
      throw new Error('project switch mock is not initialized');
    }

    await expect(canvas.findByText('OAuth 対応を実装する')).resolves.toBeInTheDocument();

    // 切替前: 作成ダイアログのラベル候補は ENG のもの。旧選択の持ち越し検証用に選択しておく
    await userEvent.click(await canvas.findByRole('button', { name: '新規タスク' }));
    const dialog = await screen.findByRole('dialog');
    const bugButton = await within(dialog).findByRole('button', { name: /bug/ });
    await userEvent.click(bugButton);
    await expect(bugButton).toHaveAttribute('aria-pressed', 'true');

    if (!reactivePageContext) {
      throw new Error('reactive page context is not initialized');
    }
    reactivePageContext.urlPathname = '/tenant-123/projects/MKT/tasks';
    reactivePageContext.routeParams.projectKey = 'MKT';
    await nextTick();

    const engTitle = 'OAuth 対応を実装する';
    const pollUntil = performance.now() + 2000;
    while (performance.now() < pollUntil) {
      await expect(canvas.queryByText(engTitle)).not.toBeInTheDocument();
      await new Promise((resolve) => setTimeout(resolve, 50));
    }

    projectSwitchMock.releaseMktTasks();
    await expect(canvas.findByText('SNSキャンペーン企画')).resolves.toBeInTheDocument();
    await expect(canvas.queryByText(engTitle)).not.toBeInTheDocument();

    // MKT の初期ロード中は一覧全体と共にダイアログも v-if で外れて unmount される。
    // 一覧の再表示で再マウントされた現在のダイアログを取り直し、ラベル候補が MKT の
    // ものに入れ替わって旧プロジェクトの候補と選択が残らないことを確認する
    const dialogAfter = await screen.findByRole('dialog');
    await expect(
      within(dialogAfter).findByText('MKT にタスクを追加します'),
    ).resolves.toBeInTheDocument();
    const campaignButton = await within(dialogAfter).findByRole('button', { name: /campaign/ });
    await expect(campaignButton).toHaveAttribute('aria-pressed', 'false');
    await expect(
      within(dialogAfter).queryByRole('button', { name: /bug/ }),
    ).not.toBeInTheDocument();
    await userEvent.click(await within(dialogAfter).findByRole('button', { name: '閉じる' }));

    // ラベルドロップダウンにも切替先プロジェクトのラベルだけが並ぶ
    // （プロジェクト一覧やENGのラベルが混入しない）
    await userEvent.click(await canvas.findByRole('button', { name: 'ラベル' }));
    const labelMenu = within(document.body);
    await expect(
      labelMenu.findByRole('menuitemradio', { name: /campaign/ }),
    ).resolves.toBeInTheDocument();
    await expect(labelMenu.queryByRole('menuitemradio', { name: /bug/ })).not.toBeInTheDocument();
    await expect(
      labelMenu.queryByRole('menuitemradio', { name: /エンジニアリング/ }),
    ).not.toBeInTheDocument();
  },
};

export const LabelFilter: Story = {
  name: 'ラベルフィルタで旧条件タスク非表示',
  beforeEach() {
    labelFilterMock = createLabelFilterMockFetch();
    return () => {
      labelFilterMock?.restore();
      labelFilterMock = null;
    };
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    if (!labelFilterMock) {
      throw new Error('label filter mock is not initialized');
    }
    const user = userEvent.setup();

    const staleTitle = 'ログイン画面の UI 実装';
    await expect(canvas.findByText(staleTitle)).resolves.toBeInTheDocument();

    await user.click(await canvas.findByRole('button', { name: 'ラベル' }));
    const menu = within(document.body);
    await user.click(await menu.findByRole('menuitemradio', { name: /bug/ }));

    // フィルタ済みレスポンスが返るまで、旧条件（未絞り込み）のタスクを表示しない
    const pollUntil = performance.now() + 1000;
    while (performance.now() < pollUntil) {
      await expect(canvas.queryByText(staleTitle)).not.toBeInTheDocument();
      await new Promise((resolve) => setTimeout(resolve, 50));
    }

    labelFilterMock.releaseFilteredTasks();
    await expect(canvas.findByText('OAuth 対応を実装する')).resolves.toBeInTheDocument();
    await expect(canvas.queryByText(staleTitle)).not.toBeInTheDocument();
  },
};

export const LabelsApiError: Story = {
  name: 'ラベル取得エラー',
  beforeEach: () => createMockFetch({ rejectLabels: true }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    // タスク一覧自体はブロックされない
    await expect(canvas.findByText('OAuth 対応を実装する')).resolves.toBeInTheDocument();
    // ラベル取得失敗はツールバー内でエラー表示＋再試行を出す
    await expect(canvas.findByText('ラベルの取得に失敗しました')).resolves.toBeInTheDocument();
    await expect(canvas.findByRole('button', { name: '再試行' })).resolves.toBeInTheDocument();
    await expect(canvas.queryByRole('button', { name: 'ラベル' })).not.toBeInTheDocument();
  },
};

export const RowAccessibility: Story = {
  name: '行の stretched link と独立コントロール',
  beforeEach() {
    const restoreFetch = mockFetch();
    return () => restoreFetch();
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);

    await expect(canvas.findByText('OAuth 対応を実装する')).resolves.toBeInTheDocument();

    const titleCell = await canvas.findByText('OAuth 対応を実装する');
    const taskLink = titleCell.closest('a');
    const taskRow = titleCell.closest('tr');
    if (!(taskLink instanceof HTMLAnchorElement)) {
      throw new Error('task link not found');
    }
    if (!(taskRow instanceof HTMLElement)) {
      throw new Error('task row not found');
    }
    await expect(taskRow).not.toHaveAttribute('role');
    await expect(taskRow).not.toHaveAttribute('tabindex');
    await expect(taskLink).toHaveAttribute('href', '/tenant-123/projects/ENG/tasks/ENG-1');
    const rowCheckbox = taskRow.querySelector('[role="checkbox"]');
    if (!(rowCheckbox instanceof HTMLElement)) {
      throw new Error('row checkbox not found');
    }
    rowCheckbox.focus();
    rowCheckbox.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true }));
    await expect(rowCheckbox).toHaveFocus();

    // 分割ビュー: 広い画面では素の左クリックで遷移せず、詳細を右ペインに inline 表示する。
    // href（ディープリンク）は残しつつ、選択で右ペインが開き URL の ?selected= に反映される。
    await userEvent.click(taskLink);
    await expect(
      canvas.findByRole('button', { name: '詳細を閉じる' }),
    ).resolves.toBeInTheDocument();
    await expect(new URLSearchParams(window.location.search).get('selected')).toBe('ENG-1');
  },
};
