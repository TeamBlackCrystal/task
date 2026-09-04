import type { Meta, StoryObj } from '@storybook/vue3-vite';
import { expect, fn, userEvent, waitFor, within } from 'storybook/test';
import { provide } from 'vue';
import { QueryClient, VUE_QUERY_CLIENT } from '@tanstack/vue-query';

import TenantSettingsPage from '@/pages/@tenant/settings/+Page.vue';

const PAGE_CONTEXT_KEY = 'vike-vue:usePageContext';

const mockContext = {
  urlPathname: '/acme-inc/settings',
  routeParams: { tenant: 'acme-inc' },
};

const TENANT_UUID = '11111111-1111-1111-1111-111111111111';

const sampleTenant = {
  id: TENANT_UUID,
  display_id: 'acme-inc',
  name: 'Acme Inc',
  description: 'プロダクト開発チーム全体のタスクを管理するテナント。',
  icon_url: '',
  owner_id: '00000000-0000-0000-0000-000000000002',
  drive_quota_bytes: null,
  require_2fa: false,
};

const jsonResponse = (data: unknown, status = 200) =>
  new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });

type MockOptions = {
  noTenant?: boolean;
  rejectWrite?: number;
  hangTenants?: boolean;
};

let fetchSpy: ReturnType<typeof fn> | null = null;

function mockFetch(overrides: MockOptions = {}) {
  return () => {
    const original = globalThis.fetch;
    fetchSpy = fn().mockImplementation(async (req: Request | string) => {
      const url = typeof req === 'string' ? req : req.url;
      const method = typeof req === 'string' ? 'GET' : req.method;
      const pathname = new URL(url, 'http://localhost').pathname;

      if (/\/v1\/tenants\/?$/.test(pathname) && method === 'GET') {
        if (overrides.hangTenants) return new Promise<Response>(() => {});
        return jsonResponse(overrides.noTenant ? [] : [sampleTenant]);
      }
      if (pathname.endsWith(`/v1/tenants/${TENANT_UUID}`)) {
        if (overrides.rejectWrite) return jsonResponse({ message: 'error' }, overrides.rejectWrite);
        if (method === 'DELETE') return new Response(null, { status: 204 });
        const body = await (req as Request).json();
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
  title: 'Pages/TenantSettings',
  component: TenantSettingsPage,
  tags: ['autodocs'],
  parameters: {
    layout: 'fullscreen',
    docs: {
      description: {
        component:
          'テナント設定の一般ページ。基本情報・既定値・アクセス・危険な操作。既定値とアクセスは API がまだ無いので表示のみ。',
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

export const AccessTogglesFlip: Story = {
  name: 'アクセスの切り替え（表示のみ）',
  beforeEach: mockFetch(),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const user = userEvent.setup();
    const toggle = await canvas.findByRole('switch', { name: 'ゲスト用の共有リンクを許す' });

    await expect(toggle).toHaveAttribute('aria-checked', 'false');
    await user.click(toggle);
    await waitFor(() => expect(toggle).toHaveAttribute('aria-checked', 'true'));
    // API に持ち場が無いので、切り替えても保存バーは出ない
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
