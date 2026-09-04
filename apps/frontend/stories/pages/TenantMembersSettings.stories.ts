import type { Meta, StoryObj } from '@storybook/vue3-vite';
import { expect, fn, userEvent, waitFor, within } from 'storybook/test';
import { provide } from 'vue';
import { QueryClient, VUE_QUERY_CLIENT } from '@tanstack/vue-query';

import TenantMembersPage from '@/pages/@tenant/settings/members/+Page.vue';

const PAGE_CONTEXT_KEY = 'vike-vue:usePageContext';

const mockContext = {
  urlPathname: '/acme-inc/settings/members',
  routeParams: { tenant: 'acme-inc' },
};

const TENANT_UUID = '11111111-1111-1111-1111-111111111111';
const OWNER_ID = '00000000-0000-0000-0000-000000000002';

const sampleTenant = {
  id: TENANT_UUID,
  display_id: 'acme-inc',
  name: 'Acme Inc',
  description: 'プロダクト開発チーム全体のタスクを管理するテナント。',
  icon_url: '',
  owner_id: OWNER_ID,
  drive_quota_bytes: null,
  require_2fa: false,
};

const member = (id: string, username: string, role: string, userId: string) => ({
  id,
  tenant_id: TENANT_UUID,
  user_id: userId,
  role,
  user: { id: userId, username, avatar_url: null },
});

const sampleMembers = [
  member('aaaaaaaa-0000-4000-8000-000000000001', 'shadcn', 'Admin', OWNER_ID),
  member(
    'aaaaaaaa-0000-4000-8000-000000000002',
    'rei.tanaka',
    'Admin',
    '00000000-0000-0000-0000-000000000003',
  ),
  member(
    'aaaaaaaa-0000-4000-8000-000000000003',
    'daisuke.sato',
    'Member',
    '00000000-0000-0000-0000-000000000004',
  ),
  member(
    'aaaaaaaa-0000-4000-8000-000000000004',
    'asuka.kobayashi',
    'Viewer',
    '00000000-0000-0000-0000-000000000005',
  ),
];

const jsonResponse = (data: unknown, status = 200) =>
  new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });

type MockOptions = {
  members?: typeof sampleMembers;
  rejectMembers?: boolean;
  hangMembers?: boolean;
  rejectWrite?: number;
};

let fetchSpy: ReturnType<typeof fn> | null = null;

function mockFetch(overrides: MockOptions = {}) {
  return () => {
    const original = globalThis.fetch;
    let current = structuredClone(overrides.members ?? sampleMembers);
    fetchSpy = fn().mockImplementation(async (req: Request | string) => {
      const url = typeof req === 'string' ? req : req.url;
      const method = typeof req === 'string' ? 'GET' : req.method;
      const pathname = new URL(url, 'http://localhost').pathname;

      if (/\/v1\/tenants\/?$/.test(pathname) && method === 'GET') {
        return jsonResponse([sampleTenant]);
      }
      if (pathname.endsWith('/v1/auth/me')) {
        return jsonResponse({ id: OWNER_ID, username: 'shadcn', avatar_url: null });
      }
      if (pathname.endsWith('/members') && method === 'GET') {
        if (overrides.hangMembers) return new Promise<Response>(() => {});
        if (overrides.rejectMembers) return jsonResponse({ message: 'error' }, 500);
        return jsonResponse(current);
      }
      if (pathname.includes('/members/')) {
        if (overrides.rejectWrite) return jsonResponse({ message: 'error' }, overrides.rejectWrite);
        const userId = pathname.split('/').at(-1)!;
        if (method === 'DELETE') {
          current = current.filter((m) => m.user_id !== userId);
          return new Response(null, { status: 204 });
        }
        const body = await (req as Request).json();
        current = current.map((m) => (m.user_id === userId ? { ...m, role: body.role } : m));
        return jsonResponse(current.find((m) => m.user_id === userId));
      }
      return jsonResponse([]);
    });
    globalThis.fetch = fetchSpy;
    return () => {
      globalThis.fetch = original;
      fetchSpy = null;
    };
  };
}

function storyDecorator() {
  return () => ({
    setup() {
      const queryClient = new QueryClient({
        defaultOptions: {
          queries: { retry: false, gcTime: 0, staleTime: 0 },
          mutations: { retry: false },
        },
      });
      provide(VUE_QUERY_CLIENT, queryClient);
      provide(PAGE_CONTEXT_KEY, mockContext);
    },
    template: '<story />',
  });
}

const meta = {
  title: 'Pages/TenantMembersSettings',
  component: TenantMembersPage,
  tags: ['autodocs'],
  parameters: {
    layout: 'fullscreen',
    docs: {
      description: {
        component:
          'テナント設定のメンバーページ。招待欄は API がまだ無いので表示のみ。一覧・ロール変更・除外は tenant_members API に繋いでいる。',
      },
    },
  },
  decorators: [storyDecorator()],
} satisfies Meta<typeof TenantMembersPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  name: 'メンバー一覧（オーナーは操作なし）',
  beforeEach: mockFetch(),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(
      canvas.findByRole('heading', { level: 1, name: 'メンバー' }),
    ).resolves.toBeInTheDocument();
    await expect(canvas.findByText('rei.tanaka')).resolves.toBeInTheDocument();

    // オーナーはロールを変えられず、外せもしない
    await expect(canvas.getByText('オーナー')).toBeInTheDocument();
    await expect(canvas.queryByLabelText('shadcnのロール')).not.toBeInTheDocument();
    await expect(canvas.queryByLabelText('shadcnを外す')).not.toBeInTheDocument();

    // オーナー以外は操作できる
    await expect(canvas.getByLabelText('rei.tanakaのロール')).toBeInTheDocument();
    await expect(canvas.getByLabelText('rei.tanakaを外す')).toBeInTheDocument();
  },
};

export const ChangeRole: Story = {
  name: 'ロール変更（PUT）',
  beforeEach: mockFetch(),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const page = within(canvasElement.ownerDocument.body);
    const user = userEvent.setup();

    await user.click(await canvas.findByLabelText('daisuke.satoのロール'));
    await user.click(await page.findByRole('option', { name: /Viewer/ }));

    const put = (fetchSpy!.mock.calls as [Request | string][])
      .map(([req]) => req)
      .filter((req): req is Request => typeof req !== 'string')
      .find((req) => req.method === 'PUT');
    await expect(put).toBeTruthy();
    await expect(put!.url).toContain('/members/00000000-0000-0000-0000-000000000004');
  },
};

export const RemoveMember: Story = {
  name: 'メンバーを外す（DELETE）',
  beforeEach: mockFetch(),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const user = userEvent.setup();

    await user.click(await canvas.findByLabelText('asuka.kobayashiを外す'));

    await waitFor(() => expect(canvas.queryByText('asuka.kobayashi')).not.toBeInTheDocument());
    const del = (fetchSpy!.mock.calls as [Request | string][])
      .map(([req]) => req)
      .filter((req): req is Request => typeof req !== 'string')
      .find((req) => req.method === 'DELETE');
    await expect(del).toBeTruthy();
  },
};

/** 最後の管理者を降格できないのは API 側の規則。理由が出ないと操作が謎に失敗する。 */
export const RoleChangeConflict: Story = {
  name: 'ロール変更が拒否される（409）',
  beforeEach: mockFetch({ rejectWrite: 409 }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const page = within(canvasElement.ownerDocument.body);
    const user = userEvent.setup();

    await user.click(await canvas.findByLabelText('rei.tanakaのロール'));
    await user.click(await page.findByRole('option', { name: /Member/ }));

    await expect(
      canvas.findByText('最後の管理者「rei.tanaka」は降格できません。'),
    ).resolves.toBeInTheDocument();
  },
};

export const RemoveError: Story = {
  name: '除外に失敗',
  beforeEach: mockFetch({ rejectWrite: 500 }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(await canvas.findByLabelText('daisuke.satoを外す'));

    await expect(canvas.findByText('メンバーを外せませんでした。')).resolves.toBeInTheDocument();
  },
};

export const OwnerOnly: Story = {
  name: 'オーナーだけ',
  beforeEach: mockFetch({ members: [sampleMembers[0]!] }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.findByText('オーナー')).resolves.toBeInTheDocument();
    await expect(canvas.getByText('1')).toBeInTheDocument();
  },
};

export const Loading: Story = {
  name: '読み込み中',
  beforeEach: mockFetch({ hangMembers: true }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.findByText('メンバーを読み込み中…')).resolves.toBeInTheDocument();
  },
};

export const LoadError: Story = {
  name: '取得エラー',
  beforeEach: mockFetch({ rejectMembers: true }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.findByText('メンバーを読み込めませんでした')).resolves.toBeInTheDocument();
  },
};
