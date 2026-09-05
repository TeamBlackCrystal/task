import type { Meta, StoryObj } from '@storybook/vue3-vite';
import { expect, fn, userEvent, waitFor, within } from 'storybook/test';
import { provide } from 'vue';
import { QueryClient, VUE_QUERY_CLIENT } from '@tanstack/vue-query';
import { createPinia, setActivePinia } from 'pinia';

import TenantMembersPage from '@/pages/@tenant/settings/members/+Page.vue';
import { useAuthStore } from '@/stores/auth';

const PAGE_CONTEXT_KEY = 'vike-vue:usePageContext';

const mockContext = {
  urlPathname: '/acme-inc/settings/members',
  routeParams: { tenant: 'acme-inc' },
};

const TENANT_UUID = '11111111-1111-1111-1111-111111111111';
const OWNER_ID = '00000000-0000-0000-0000-000000000002';
const MEMBERS_PATH = '/v1/tenants/{tenant_id}/members';

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
  me?: { id: string; username: string };
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
      if (pathname.includes('/v1/auth/me')) {
        const currentUser = overrides.me ?? { id: OWNER_ID, username: 'shadcn' };
        return jsonResponse({ ...currentUser, avatar_url: null });
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

type StoryUser = { id: string; username: string; avatar_url: null };
type StoryParameters = {
  currentUser?: StoryUser;
  preloadMembers?: boolean;
};

function storyDecorator() {
  return (_story: unknown, context: { parameters?: StoryParameters }) => ({
    setup() {
      setActivePinia(createPinia());
      const currentUser = context.parameters?.currentUser ?? {
        id: OWNER_ID,
        username: 'shadcn',
        avatar_url: null,
      };
      useAuthStore().setUser({
        ...currentUser,
        email: `${currentUser.username}@example.com`,
        email_verified: true,
        is_admin: false,
        is_suspended: false,
        totp_enabled: false,
      });
      const queryClient = new QueryClient({
        defaultOptions: {
          queries: { retry: false, gcTime: 0, staleTime: 0 },
          mutations: { retry: false },
        },
      });
      queryClient.setQueryData(['get', '/v1/auth/me'], currentUser);
      if (context.parameters?.preloadMembers) {
        queryClient.setQueryData(['get', '/v1/tenants'], [sampleTenant]);
        queryClient.setQueryData(
          ['get', MEMBERS_PATH, { params: { path: { tenant_id: TENANT_UUID } } }],
          sampleMembers,
        );
      }
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
          'テナント設定のメンバーページ。owner は API の synthetic row と、旧レスポンス向けの本人情報 fallback で表示する。招待は未実装で、一覧・ロール変更・除外は tenant_members API に繋いでいる。',
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

/** API が返すエラーを、実装されていない規則に読み替えずそのまま表示する。 */
export const RoleChangeError: Story = {
  name: 'ロール変更に失敗',
  beforeEach: mockFetch({ rejectWrite: 409 }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const page = within(canvasElement.ownerDocument.body);
    const user = userEvent.setup();

    await user.click(await canvas.findByLabelText('rei.tanakaのロール'));
    await user.click(await page.findByRole('option', { name: /Member/ }));

    await expect(canvas.findByText('ロールを変更できませんでした。')).resolves.toBeInTheDocument();
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
  beforeEach: mockFetch({ members: [] }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.findByText('オーナー')).resolves.toBeInTheDocument();
    await expect(canvas.getByText('1')).toBeInTheDocument();
  },
};

export const MemberReadOnly: Story = {
  name: 'Member は閲覧のみ',
  parameters: {
    preloadMembers: true,
    currentUser: {
      id: '00000000-0000-0000-0000-000000000004',
      username: 'daisuke.sato',
      avatar_url: null,
    },
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.findByLabelText('rei.tanakaのロール')).resolves.toBeDisabled();
    await expect(canvas.findByLabelText('rei.tanakaを外す')).resolves.toBeDisabled();
    await expect(canvas.getByRole('button', { name: '招待' })).toBeDisabled();
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
