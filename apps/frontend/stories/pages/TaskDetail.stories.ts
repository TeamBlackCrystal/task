import type { Meta, StoryObj } from '@storybook/vue3-vite';
import { expect, fn, userEvent, within } from 'storybook/test';
import { provide } from 'vue';
import { QueryClient, VUE_QUERY_CLIENT } from '@tanstack/vue-query';
import TaskDetailPage from '@/pages/@tenant/projects/@projectKey/tasks/@taskId/+Page.vue';

const PAGE_CONTEXT_KEY = 'vike-vue:usePageContext';
// vike-vue の useData は inject('vike-vue:useData') の素通し。+data.ts の戻り値と
// 同形のオブジェクトを provide すると +Page.vue の SSR データ枝 (KFM 表示) が通る。
const DATA_KEY = 'vike-vue:useData';

const mockContext = {
  urlPathname: '/tenant-123/projects/ENG/tasks/ENG-1',
  routeParams: { tenant: 'tenant-123', projectKey: 'ENG', taskId: 'ENG-1' },
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
} as const;

const sampleProjects = [
  {
    id: 'proj-eng',
    key: 'ENG',
    name: 'エンジニアリング',
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
    id: 's-done',
    name: 'Done',
    color: '#22c55e',
    position: 2,
    is_default: false,
    is_done_state: true,
    project_id: 'proj-eng',
    created_at: '2026-01-01T00:00:00Z',
  },
];

const sampleTaskDetail = {
  id: 'task-1',
  seq_id: 1,
  title: 'OAuth 対応を実装する',
  description: 'OIDC フローとセッション管理を実装する。' as string | null,
  priority: 'High' as const,
  status_id: 's-progress',
  project_id: 'proj-eng',
  soft_deadline: '2026-07-02T00:00:00Z' as string | null,
  hard_deadline: null as string | null,
  is_archived: false,
  progress_pct: 30,
  created_at: '2026-06-01T00:00:00Z',
  updated_at: '2026-06-15T00:00:00Z',
  created_by: mockUsers.alpha,
  assignees: [
    { role: 'assignee', user: mockUsers.alpha },
    { role: 'assignee', user: mockUsers.beta },
  ],
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
  custom_field_values: [],
};

const sampleLabels = [
  ...sampleTaskDetail.labels,
  {
    id: 'label-feature',
    name: 'feature',
    description: '',
    color: '#3b82f6',
    icon_url: null,
    project_id: 'proj-eng',
  },
];

type MockOptions = {
  task?: typeof sampleTaskDetail | null;
  rejectAll?: boolean;
  rejectTenantsList?: boolean;
  rejectPut?: number;
  rejectDelete?: number;
  deleteResponse?: Promise<Response>;
  hang?: boolean;
  onPut?: (body: unknown) => void;
  onDelete?: () => void;
};

function applyPutBody(
  task: typeof sampleTaskDetail,
  body: Record<string, unknown>,
): typeof sampleTaskDetail {
  const next = { ...task };

  if (body.clear_description) next.description = null;
  else if (typeof body.description === 'string') next.description = body.description;

  if (body.clear_soft_deadline) next.soft_deadline = null;
  else if (typeof body.soft_deadline === 'string') next.soft_deadline = body.soft_deadline;

  if (body.clear_hard_deadline) next.hard_deadline = null;
  else if (typeof body.hard_deadline === 'string') next.hard_deadline = body.hard_deadline;

  if (typeof body.progress_pct === 'number') next.progress_pct = body.progress_pct;
  if (typeof body.title === 'string') next.title = body.title;
  if (typeof body.status_id === 'string') next.status_id = body.status_id;

  if (Array.isArray(body.label_ids)) {
    next.labels = sampleLabels.filter((label) => (body.label_ids as string[]).includes(label.id));
  }

  return next;
}

let mutableTaskDetail = { ...sampleTaskDetail };

function createMockFetch(overrides: MockOptions = {}) {
  mutableTaskDetail = { ...(overrides.task ?? sampleTaskDetail) };
  const original = globalThis.fetch;
  globalThis.fetch = fn().mockImplementation(async (req: Request) => {
    const url = typeof req === 'string' ? req : req.url;
    if (isListTenantsUrl(url)) {
      if (overrides.rejectTenantsList) {
        return jsonResponse({ message: 'server error' }, 500);
      }
      return jsonResponse(sampleTenants(mockContext.routeParams.tenant));
    }
    if (overrides.rejectAll) throw new TypeError('Failed to fetch');
    if (overrides.hang) return new Promise(() => {});

    const method = typeof req === 'string' ? 'GET' : req.method;
    if (url.includes('/statuses')) {
      return jsonResponse(sampleStatuses);
    }
    if (url.includes('/labels')) {
      return jsonResponse(sampleLabels);
    }
    if (method === 'PUT' && url.includes('/tasks/')) {
      if (overrides.rejectPut) {
        return jsonResponse({ message: 'update failed' }, overrides.rejectPut);
      }
      const body = (await req.json()) as Record<string, unknown>;
      overrides.onPut?.(body);
      mutableTaskDetail = applyPutBody(mutableTaskDetail, body);
      return jsonResponse(mutableTaskDetail);
    }
    if (method === 'DELETE' && url.includes('/tasks/')) {
      if (overrides.deleteResponse) return overrides.deleteResponse;
      if (overrides.rejectDelete) {
        return jsonResponse({ message: 'delete failed' }, overrides.rejectDelete);
      }
      overrides.onDelete?.();
      return new Response(null, { status: 204 });
    }
    if (url.includes('/tasks/')) {
      if (overrides.task === null) {
        return jsonResponse({ message: 'not-found' }, 404);
      }
      return jsonResponse(overrides.task ?? mutableTaskDetail);
    }
    // tasks 一覧はこのストーリーでは未使用。projects の分岐に吸われないようここで受ける
    if (url.includes('/tasks')) {
      return jsonResponse({});
    }
    // 残ったプロジェクト系だけを最後に受ける
    if (url.includes('/v1/tenants/') && url.includes('/projects')) {
      return jsonResponse(sampleProjects);
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

function storyDecorator(
  context: { urlPathname: string; routeParams: Record<string, string> } = mockContext,
  pageData?: { descriptionHtml: string | null; descriptionSource: string | null },
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
      if (pageData) provide(DATA_KEY, pageData);
    },
    template: '<story />',
  });
}

async function openDeleteDialogFromKebabMenu(
  canvas: ReturnType<typeof within>,
  user: ReturnType<typeof userEvent.setup>,
) {
  await user.click(canvas.getByRole('button', { name: 'タスク操作' }));
  const menu = within(document.body);
  await user.click(await menu.findByRole('menuitem', { name: '削除' }));
}

const meta = {
  title: 'Pages/TaskDetail',
  component: TaskDetailPage,
  tags: ['autodocs'],
  parameters: {
    layout: 'padded',
    docs: {
      description: {
        component:
          'タスク詳細ハブ（増分2）。GET 表示・ステータス/フィールドのインライン編集・ソフト削除・loading/404/error を fetch モックで検証。',
      },
    },
  },
  decorators: [storyDecorator()],
} satisfies Meta<typeof TaskDetailPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  name: '詳細表示',
  beforeEach: mockFetch,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(
      canvas.findByRole('heading', { name: 'OAuth 対応を実装する' }),
    ).resolves.toBeInTheDocument();
    await expect(canvas.findByText('ENG-1')).resolves.toBeInTheDocument();
    await expect(
      canvas.findByText('OIDC フローとセッション管理を実装する。'),
    ).resolves.toBeInTheDocument();
    // 担当者はアバター（頭文字）のみ表示し、名前テキストは出さない（詳細では hideNames）
    await expect(canvas.findByText('田')).resolves.toBeInTheDocument();
    await expect(canvas.queryByText('田中太郎')).not.toBeInTheDocument();
  },
};

// KFM 表示 story の入力対。本番では +data.ts (サーバ) の renderDescription 出力だが、
// story は同期描画のため sanitize 済み出力と同形の静的 HTML を使う。
// descriptionSource は task.description と厳密一致させる (照合が成立する条件)。
const KFM_DESCRIPTION = '**強調** と `code` を含む説明';
const KFM_DESCRIPTION_HTML = '<p><strong>強調</strong> と <code>code</code> を含む説明</p>';

export const DescriptionKfmRendered: Story = {
  name: '説明 KFM 描画（useData 接続）',
  decorators: [
    storyDecorator(mockContext, {
      descriptionHtml: KFM_DESCRIPTION_HTML,
      descriptionSource: KFM_DESCRIPTION,
    }),
  ],
  beforeEach: () =>
    createMockFetch({ task: { ...sampleTaskDetail, description: KFM_DESCRIPTION } }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    // 照合成立 (descriptionSource === task.description) → KFM 枝が v-html 描画される
    const strong = await canvas.findByText('強調');
    expect(strong.tagName).toBe('STRONG');
    // プレーン枝 (生テキストのマーカー記法そのまま) が出ていない
    expect(canvas.queryByText(KFM_DESCRIPTION)).toBeNull();
  },
};

export const NotFound: Story = {
  name: 'タスクなし',
  beforeEach: () => createMockFetch({ task: null }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.findByText('タスクが見つかりません')).resolves.toBeInTheDocument();
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
    expect(canvas.queryByText('タスクが見つかりません')).toBeNull();
  },
};

export const Loading: Story = {
  name: 'ロード中',
  beforeEach: () => createMockFetch({ hang: true }),
};

export const StatusChange: Story = {
  name: 'ステータス変更',
  beforeEach: () => createMockFetch(),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const user = userEvent.setup();
    await expect(
      canvas.findByRole('heading', { name: 'OAuth 対応を実装する' }),
    ).resolves.toBeInTheDocument();

    const select = await canvas.findByRole('combobox', { name: 'ステータス' });
    await user.selectOptions(select, 's-done');
    await expect(select).toHaveValue('s-done');
  },
};

export const StatusChangeFailure500: Story = {
  name: 'ステータス変更失敗（500）',
  beforeEach: () => createMockFetch({ rejectPut: 500 }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const user = userEvent.setup();
    await expect(
      canvas.findByRole('heading', { name: 'OAuth 対応を実装する' }),
    ).resolves.toBeInTheDocument();

    const select = await canvas.findByRole('combobox', { name: 'ステータス' });
    await expect(select).toHaveValue('s-progress');
    await user.selectOptions(select, 's-done');
    await expect(canvas.findByText('ステータスの更新に失敗しました')).resolves.toBeInTheDocument();
    await expect(select).toHaveValue('s-progress');
  },
};

export const StatusChangeFailure413: Story = {
  name: 'ステータス変更失敗（413）',
  beforeEach: () => createMockFetch({ rejectPut: 413 }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const user = userEvent.setup();
    await expect(
      canvas.findByRole('heading', { name: 'OAuth 対応を実装する' }),
    ).resolves.toBeInTheDocument();

    const select = await canvas.findByRole('combobox', { name: 'ステータス' });
    await expect(select).toHaveValue('s-progress');
    await user.selectOptions(select, 's-done');
    await expect(canvas.findByText('ステータスの更新に失敗しました')).resolves.toBeInTheDocument();
    await expect(select).toHaveValue('s-progress');
  },
};

export const TitleEdit: Story = {
  name: 'タイトル編集',
  beforeEach: () => {
    const puts: unknown[] = [];
    const restore = createMockFetch({
      onPut: (body) => puts.push(body),
    });
    (TitleEdit as { puts?: unknown[] }).puts = puts;
    return restore;
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const user = userEvent.setup();
    await expect(
      canvas.findByRole('heading', { name: 'OAuth 対応を実装する' }),
    ).resolves.toBeInTheDocument();

    await user.click(canvas.getByRole('heading', { name: 'OAuth 対応を実装する' }));
    const input = await canvas.findByRole('textbox', { name: 'タイトル' });
    await user.clear(input);
    await user.type(input, '新しいタイトル');
    await user.tab();

    await expect(
      canvas.findByRole('heading', { name: '新しいタイトル' }),
    ).resolves.toBeInTheDocument();
    const puts = (TitleEdit as { puts?: unknown[] }).puts ?? [];
    await expect(puts).toContainEqual({ title: '新しいタイトル' });
  },
};

export const DescriptionEdit: Story = {
  name: '説明編集',
  beforeEach: () => {
    const puts: unknown[] = [];
    const restore = createMockFetch({
      onPut: (body) => puts.push(body),
    });
    (DescriptionEdit as { puts?: unknown[] }).puts = puts;
    return restore;
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const user = userEvent.setup();
    await expect(
      canvas.findByText('OIDC フローとセッション管理を実装する。'),
    ).resolves.toBeInTheDocument();

    await user.click(canvas.getByText('OIDC フローとセッション管理を実装する。'));
    const textarea = await canvas.findByRole('textbox', { name: '説明' });
    await user.clear(textarea);
    await user.type(textarea, '更新後の説明');
    await user.tab();

    await expect(canvas.findByText('更新後の説明')).resolves.toBeInTheDocument();
    const puts = (DescriptionEdit as { puts?: unknown[] }).puts ?? [];
    await expect(puts).toContainEqual({ description: '更新後の説明' });
  },
};

export const DescriptionClear: Story = {
  name: '説明クリア',
  beforeEach: () => {
    const puts: unknown[] = [];
    const restore = createMockFetch({
      onPut: (body) => puts.push(body),
    });
    (DescriptionClear as { puts?: unknown[] }).puts = puts;
    return restore;
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const user = userEvent.setup();
    await expect(
      canvas.findByText('OIDC フローとセッション管理を実装する。'),
    ).resolves.toBeInTheDocument();

    await user.click(canvas.getByText('OIDC フローとセッション管理を実装する。'));
    await user.click(await canvas.findByRole('button', { name: 'クリア' }));

    await expect(
      canvas.findByText('説明はありません（クリックして追加）'),
    ).resolves.toBeInTheDocument();
    const puts = (DescriptionClear as { puts?: unknown[] }).puts ?? [];
    await expect(puts).toContainEqual({ clear_description: true });
  },
};

export const ProgressEdit: Story = {
  name: '進捗編集',
  beforeEach: () => {
    const puts: unknown[] = [];
    const restore = createMockFetch({
      onPut: (body) => puts.push(body),
    });
    (ProgressEdit as { puts?: unknown[] }).puts = puts;
    return restore;
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const user = userEvent.setup();
    await expect(canvas.findByText('30%')).resolves.toBeInTheDocument();

    await user.click(canvas.getByText('30%'));
    const input = await canvas.findByRole('spinbutton', { name: '進捗率' });
    await user.clear(input);
    await user.type(input, '75');
    await user.tab();

    await expect(canvas.findByText('75%')).resolves.toBeInTheDocument();
    const puts = (ProgressEdit as { puts?: unknown[] }).puts ?? [];
    await expect(puts).toContainEqual({ progress_pct: 75 });
  },
};

export const SoftDeadlineSet: Story = {
  name: 'ソフト期限設定',
  beforeEach: () => {
    const puts: unknown[] = [];
    const restore = createMockFetch({
      task: { ...sampleTaskDetail, soft_deadline: null },
      onPut: (body) => puts.push(body),
    });
    (SoftDeadlineSet as { puts?: unknown[] }).puts = puts;
    return restore;
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const user = userEvent.setup();

    await user.click(await canvas.findByRole('button', { name: 'ソフト期限を編集' }));
    const input = await canvas.findByLabelText('ソフト期限');
    await user.type(input, '2026-08-23');
    await user.tab();

    const puts = (SoftDeadlineSet as { puts?: unknown[] }).puts ?? [];
    await expect(puts).toContainEqual({ soft_deadline: '2026-08-23T00:00:00.000Z' });
  },
};

export const SoftDeadlineClear: Story = {
  name: 'ソフト期限クリア',
  beforeEach: () => {
    const puts: unknown[] = [];
    const restore = createMockFetch({
      onPut: (body) => puts.push(body),
    });
    (SoftDeadlineClear as { puts?: unknown[] }).puts = puts;
    return restore;
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const user = userEvent.setup();
    await expect(canvas.findByText('ソフト期限')).resolves.toBeInTheDocument();

    const row = canvas.getByText('ソフト期限').parentElement;
    expect(row).toBeTruthy();
    const section = within(row!);
    await user.click(section.getByRole('button'));
    const input = await section.findByLabelText('ソフト期限');
    await user.clear(input);
    await user.tab();

    await expect(section.findByText('未設定（クリックして設定）')).resolves.toBeInTheDocument();
    const puts = (SoftDeadlineClear as { puts?: unknown[] }).puts ?? [];
    await expect(puts).toContainEqual({ clear_soft_deadline: true });
  },
};

export const FieldEditFailureRollback: Story = {
  name: 'フィールド編集失敗ロールバック',
  beforeEach: () => createMockFetch({ rejectPut: 500 }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const user = userEvent.setup();
    await expect(canvas.findByText('30%')).resolves.toBeInTheDocument();

    await user.click(canvas.getByText('30%'));
    const input = await canvas.findByRole('spinbutton', { name: '進捗率' });
    await user.clear(input);
    await user.type(input, '80');
    await user.tab();

    await expect(canvas.findByText('更新に失敗しました')).resolves.toBeInTheDocument();
    await expect(canvas.findByText('30%')).resolves.toBeInTheDocument();
  },
};

export const DeleteConfirmAndNavigate: Story = {
  name: '削除確認→204→一覧遷移',
  decorators: [
    () => ({
      setup() {
        const queryClient = new QueryClient({
          defaultOptions: {
            queries: { retry: false, gcTime: 0, staleTime: 0 },
            mutations: { retry: false },
          },
        });
        const spaNavigateSpy = fn();
        provide(VUE_QUERY_CLIENT, queryClient);
        provide(PAGE_CONTEXT_KEY, mockContext);
        provide('navigateAfterDelete', (href: string) => {
          spaNavigateSpy(href);
        });
        (DeleteConfirmAndNavigate as { spaNavigateSpy?: ReturnType<typeof fn> }).spaNavigateSpy =
          spaNavigateSpy;
      },
      template: '<story />',
    }),
  ],
  beforeEach: () => {
    let deleted = false;
    const restore = createMockFetch({
      onDelete: () => {
        deleted = true;
      },
    });
    (DeleteConfirmAndNavigate as { wasDeleted?: () => boolean }).wasDeleted = () => deleted;
    return restore;
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const user = userEvent.setup();
    await expect(
      canvas.findByRole('heading', { name: 'OAuth 対応を実装する' }),
    ).resolves.toBeInTheDocument();

    await openDeleteDialogFromKebabMenu(canvas, user);
    const dialog = await canvas.findByRole('dialog');
    await expect(
      within(dialog).findByRole('heading', { name: 'タスクを削除しますか？' }),
    ).resolves.toBeInTheDocument();
    await user.click(within(dialog).getByRole('button', { name: '削除する' }));

    const spaNavigateSpy = (DeleteConfirmAndNavigate as { spaNavigateSpy?: ReturnType<typeof fn> })
      .spaNavigateSpy;
    await expect(spaNavigateSpy).toHaveBeenCalledWith('/tenant-123/projects/ENG/tasks');
    expect((DeleteConfirmAndNavigate as { wasDeleted?: () => boolean }).wasDeleted?.()).toBe(true);
  },
};

export const DeleteInFlightEscapeGuard: Story = {
  name: '削除中は Escape で閉じず失敗を表示',
  beforeEach: () => {
    let resolveDelete: ((response: Response) => void) | undefined;
    const deleteResponse = new Promise<Response>((resolve) => {
      resolveDelete = resolve;
    });
    (DeleteInFlightEscapeGuard as { rejectDelete?: () => void }).rejectDelete = () => {
      resolveDelete?.(jsonResponse({ message: 'delete failed' }, 500));
    };
    return createMockFetch({ deleteResponse });
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const user = userEvent.setup();
    await expect(
      canvas.findByRole('heading', { name: 'OAuth 対応を実装する' }),
    ).resolves.toBeInTheDocument();

    await openDeleteDialogFromKebabMenu(canvas, user);
    const dialog = await canvas.findByRole('dialog');
    await user.click(within(dialog).getByRole('button', { name: '削除する' }));
    await expect(within(dialog).findByRole('button', { name: '削除中…' })).resolves.toBeDisabled();

    const cancelEvent = new Event('cancel', { cancelable: true });
    dialog.dispatchEvent(cancelEvent);
    expect(cancelEvent.defaultPrevented).toBe(true);
    expect(dialog).toHaveAttribute('open');

    (DeleteInFlightEscapeGuard as { rejectDelete?: () => void }).rejectDelete?.();
    await expect(
      within(dialog).findByText('タスクの削除に失敗しました'),
    ).resolves.toBeInTheDocument();
    expect(dialog).toHaveAttribute('open');
  },
};

export const DeleteCancel: Story = {
  name: '削除キャンセル',
  beforeEach: () => createMockFetch(),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const user = userEvent.setup();
    await expect(
      canvas.findByRole('heading', { name: 'OAuth 対応を実装する' }),
    ).resolves.toBeInTheDocument();

    await openDeleteDialogFromKebabMenu(canvas, user);
    const dialog = await canvas.findByRole('dialog');
    await user.click(within(dialog).getByRole('button', { name: 'キャンセル' }));

    await expect(canvas.queryByRole('dialog')).toBeNull();
    await expect(canvas.getByRole('heading', { name: 'OAuth 対応を実装する' })).toBeInTheDocument();
  },
};

export const DeleteFailure: Story = {
  name: '削除失敗',
  beforeEach: () => createMockFetch({ rejectDelete: 500 }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const user = userEvent.setup();
    await expect(
      canvas.findByRole('heading', { name: 'OAuth 対応を実装する' }),
    ).resolves.toBeInTheDocument();

    await openDeleteDialogFromKebabMenu(canvas, user);
    const dialog = await canvas.findByRole('dialog');
    await user.click(within(dialog).getByRole('button', { name: '削除する' }));

    await expect(
      within(dialog).findByText('タスクの削除に失敗しました'),
    ).resolves.toBeInTheDocument();
    await expect(canvas.getByRole('heading', { name: 'OAuth 対応を実装する' })).toBeInTheDocument();
  },
};
