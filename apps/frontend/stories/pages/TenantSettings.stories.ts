import type { Meta, StoryObj } from '@storybook/vue3-vite';
import { expect, fn, userEvent, waitFor, within } from 'storybook/test';
import { provide } from 'vue';
import { QueryClient, VUE_QUERY_CLIENT } from '@tanstack/vue-query';
import { createPinia, setActivePinia } from 'pinia';

import TenantSettingsPage from '@/pages/@tenant/settings/+Page.vue';
import { useAuthStore } from '@/stores/auth';

const PAGE_CONTEXT_KEY = 'vike-vue:usePageContext';

const mockContext = {
  urlPathname: '/acme-inc/settings',
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

const jsonResponse = (data: unknown, status = 200) =>
  new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });

type MockOptions = {
  me?: { id: string; username: string };
  noTenant?: boolean;
  rejectWrite?: number;
  hangTenants?: boolean;
};

let fetchSpy: ReturnType<typeof fn> | null = null;
/** PUT の body。mock 側が Request を読み切るので、送った中身はここに控える。 */
let putBodies: unknown[] = [];

function mockFetch(overrides: MockOptions = {}) {
  return () => {
    const original = globalThis.fetch;
    putBodies = [];
    fetchSpy = fn().mockImplementation(async (req: Request | string) => {
      const url = typeof req === 'string' ? req : req.url;
      const method = typeof req === 'string' ? 'GET' : req.method;
      const pathname = new URL(url, 'http://localhost').pathname;

      if (/\/v1\/tenants\/?$/.test(pathname) && method === 'GET') {
        if (overrides.hangTenants) return new Promise<Response>(() => {});
        return jsonResponse(overrides.noTenant ? [] : [sampleTenant]);
      }
      if (pathname.includes('/v1/auth/me')) {
        return jsonResponse(overrides.me ?? { id: OWNER_ID, username: 'shadcn', avatar_url: null });
      }
      if (pathname.endsWith(`/v1/tenants/${TENANT_UUID}`)) {
        if (overrides.rejectWrite) return jsonResponse({ message: 'error' }, overrides.rejectWrite);
        if (method === 'DELETE') return new Response(null, { status: 204 });
        const body = await (req as Request).json();
        if (method === 'PUT') putBodies.push(body);
        return jsonResponse({ ...sampleTenant, ...body });
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

function storyDecorator() {
  return (_story: unknown, context: { parameters?: { currentUser?: StoryUser } }) => ({
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
      provide(VUE_QUERY_CLIENT, queryClient);
      provide(PAGE_CONTEXT_KEY, mockContext);
    },
    template: '<story />',
  });
}

const meta = {
  title: 'Pages/TenantSettings',
  component: TenantSettingsPage,
  tags: ['autodocs'],
  parameters: {
    layout: 'fullscreen',
    docs: {
      description: {
        component:
          'テナント設定の一般ページ。基本情報は owner のみ編集できる。既定値・アクセス・owner 移譲は未実装のため表示のみで、削除は owner のみ実行できる。',
      },
    },
  },
  decorators: [storyDecorator()],
} satisfies Meta<typeof TenantSettingsPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  name: '設定表示（プリフィル・URL は変更不可）',
  beforeEach: mockFetch(),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.findByRole('heading', { name: '一般' })).resolves.toBeInTheDocument();
    await expect(canvas.findByLabelText('テナント名')).resolves.toHaveValue('Acme Inc');
    await expect(canvas.getByLabelText('URL')).toBeDisabled();
    await expect(canvas.getByLabelText('URL')).toHaveValue('acme-inc');
    // 変更していないので保存バーは出ない
    await expect(canvas.queryByText('保存されていない変更があります')).not.toBeInTheDocument();
  },
};

export const SaveBarAppearsOnEdit: Story = {
  name: '編集すると保存バーが出る → 保存（PUT）',
  beforeEach: mockFetch(),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const user = userEvent.setup();
    const name = await canvas.findByLabelText('テナント名');
    await user.clear(name);
    await user.type(name, 'Acme Corp');

    await expect(canvas.findByText('保存されていない変更があります')).resolves.toBeInTheDocument();
    await user.click(canvas.getByRole('button', { name: '変更を保存' }));

    const put = (fetchSpy!.mock.calls as [Request | string][])
      .map(([req]) => req)
      .filter((req): req is Request => typeof req !== 'string')
      .find((req) => req.method === 'PUT');
    await expect(put).toBeTruthy();
    await expect(put!.url).toContain(`/v1/tenants/${TENANT_UUID}`);
  },
};

/**
 * 絵文字は選んでも保存対象に入っておらず、画面を離れると黙って消えていた。
 * 保存バーが出ること、PUT の body に載ることの両方を固定する。
 */
export const EmojiSelectionIsSaved: Story = {
  name: '絵文字を選ぶと保存バーが出る → PUT に icon_emoji が載る',
  beforeEach: mockFetch(),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    // ピッカーの中身は DropdownMenuPortal で canvas の外に出る
    const page = within(canvasElement.ownerDocument.body);
    const user = userEvent.setup();

    await user.click(await canvas.findByRole('button', { name: 'テナントアイコンを変更' }));
    await user.click(await page.findByRole('button', { name: 'アイコン 🚀' }));
    // 選択肢は素の button なのでメニューは開いたまま。閉じないと背後が aria-hidden で触れない
    await user.keyboard('{Escape}');
    await waitFor(() => expect(page.queryByText('絵文字を選択')).not.toBeInTheDocument());

    await expect(canvas.findByText('保存されていない変更があります')).resolves.toBeInTheDocument();
    await user.click(canvas.getByRole('button', { name: '変更を保存' }));

    await expect(putBodies.at(-1)).toMatchObject({ icon_emoji: '🚀' });
  },
};

export const DiscardRestoresValues: Story = {
  name: '取り消すと元に戻る',
  beforeEach: mockFetch(),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const user = userEvent.setup();
    const name = await canvas.findByLabelText('テナント名');
    await user.clear(name);
    await user.type(name, '書きかけ');
    await user.click(await canvas.findByRole('button', { name: '取り消す' }));

    await waitFor(() => expect(canvas.getByLabelText('テナント名')).toHaveValue('Acme Inc'));
    await expect(canvas.queryByText('保存されていない変更があります')).not.toBeInTheDocument();
  },
};

/** 名前を空にしたまま保存できると、API に弾かれるだけで理由が分からない。 */
export const EmptyNameBlocksSave: Story = {
  name: '名前が空なら保存できない',
  beforeEach: mockFetch(),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const user = userEvent.setup();
    await user.clear(await canvas.findByLabelText('テナント名'));

    await expect(canvas.findByRole('button', { name: '変更を保存' })).resolves.toBeDisabled();
  },
};

export const MemberCannotEdit: Story = {
  name: 'Member は編集できない',
  parameters: {
    currentUser: {
      id: '00000000-0000-0000-0000-000000000004',
      username: 'daisuke.sato',
      avatar_url: null,
    },
  },
  beforeEach: mockFetch({
    me: {
      id: '00000000-0000-0000-0000-000000000004',
      username: 'daisuke.sato',
    },
  }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.findByLabelText('テナント名')).resolves.toBeDisabled();
    await expect(canvas.getByLabelText('説明')).toBeDisabled();
    await expect(canvas.findByRole('status')).resolves.toHaveTextContent('テナントオーナーだけ');
    await expect(canvas.queryByRole('button', { name: '削除' })).not.toBeInTheDocument();
  },
};

export const SaveError: Story = {
  name: '保存に失敗',
  beforeEach: mockFetch({ rejectWrite: 500 }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const user = userEvent.setup();
    const name = await canvas.findByLabelText('テナント名');
    await user.clear(name);
    await user.type(name, 'Acme Corp');
    await user.click(canvas.getByRole('button', { name: '変更を保存' }));

    await expect(canvas.findByText('テナントを更新できませんでした')).resolves.toBeInTheDocument();
  },
};

/** 打ち間違いで消えないよう、URL と同じ文字列を打つまで削除できない。 */
export const DeleteNeedsTypedSlug: Story = {
  name: '削除は URL を打つまで押せない',
  beforeEach: mockFetch(),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const page = within(canvasElement.ownerDocument.body);
    const user = userEvent.setup();

    await user.click(await canvas.findByRole('button', { name: '削除' }));
    await expect(
      page.findByRole('heading', { name: '「Acme Inc」を削除しますか？' }),
    ).resolves.toBeInTheDocument();

    const confirm = page.getByRole('button', { name: 'テナントを削除' });
    await expect(confirm).toBeDisabled();

    await user.type(page.getByLabelText(/確認のため/), 'acme-inc');
    await waitFor(() => expect(confirm).toBeEnabled());
    await user.click(confirm);

    const del = (fetchSpy!.mock.calls as [Request | string][])
      .map(([req]) => req)
      .filter((req): req is Request => typeof req !== 'string')
      .find((req) => req.method === 'DELETE');
    await expect(del).toBeTruthy();
    await expect(del!.url).toContain(`/v1/tenants/${TENANT_UUID}`);
  },
};

export const AccessTogglesDisabled: Story = {
  name: 'アクセスの切り替え（未実装）',
  beforeEach: mockFetch(),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const toggle = await canvas.findByRole('switch', { name: 'ゲスト用の共有リンクを許す' });

    await expect(toggle).toHaveAttribute('aria-checked', 'false');
    await expect(toggle).toBeDisabled();
    // API ができるまでは表示だけで状態は変えない
    await expect(canvas.queryByText('保存されていない変更があります')).not.toBeInTheDocument();
  },
};

export const Loading: Story = {
  name: '読み込み中',
  beforeEach: mockFetch({ hangTenants: true }),
};

export const NotFound: Story = {
  name: 'テナントなし',
  beforeEach: mockFetch({ noTenant: true }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.findByText('テナントが見つかりません')).resolves.toBeInTheDocument();
  },
};
