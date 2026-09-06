import type { Meta, StoryObj } from '@storybook/vue3-vite';
import { QueryClient, VUE_QUERY_CLIENT } from '@tanstack/vue-query';
import { createPinia, setActivePinia } from 'pinia';
import { expect, fn, userEvent, within } from 'storybook/test';
import { provide } from 'vue';

import TenantSwitcher from '@/components/header/TenantSwitcher.vue';
import type { Tenant } from '@/stores/tenant';
import { useTenantStore } from '@/stores/tenant';

const ownerId = '00000000-0000-4000-8000-000000000001';

const tenant = (id: string, name: string, displayId: string): Tenant => ({
  id,
  name,
  display_id: displayId,
  description: `${name} tenant`,
  icon_url: '',
  owner_id: ownerId,
  require_2fa: false,
});

const primaryTenant = tenant('00000000-0000-4000-8000-000000000010', 'Acme', 'acme');
const secondaryTenant = tenant('00000000-0000-4000-8000-000000000020', 'Globex', 'globex');
const retry = fn();

/** ドロップダウンの頭に出すメンバー数のぶんだけ応答を用意する。 */
function mockMembers(count = 3) {
  return () => {
    const original = globalThis.fetch;
    globalThis.fetch = (async (req: Request | string) => {
      const url = typeof req === 'string' ? req : req.url;
      const body = url.includes('/v1/auth/me')
        ? { id: ownerId, username: 'yupix', avatar_url: null }
        : url.includes('/members')
          ? Array.from({ length: count }, (_, index) => ({ id: String(index) }))
          : [];
      return new Response(JSON.stringify(body), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      });
    }) as typeof globalThis.fetch;
    return () => {
      globalThis.fetch = original;
    };
  };
}

type TenantState = {
  tenants: Tenant[];
  selectedTenantId: string | null;
  isLoading?: boolean;
  error?: string | null;
};

const renderWithStore = (state: TenantState) => () => ({
  components: { TenantSwitcher },
  setup() {
    const pinia = createPinia();
    setActivePinia(pinia);
    // メンバー数を引くので、クエリの土台が要る
    provide(
      VUE_QUERY_CLIENT,
      new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0, staleTime: 0 } } }),
    );
    const store = useTenantStore();
    store.$patch({
      tenants: state.tenants,
      selectedTenantId: state.selectedTenantId,
      isLoading: state.isLoading ?? false,
      error: state.error ?? null,
    });

    function selectTenant(selected: Tenant) {
      store.selectTenant(selected);
    }

    function retryLoad() {
      retry();
    }

    return { retryLoad, selectTenant, store };
  },
  template: `
    <div class="w-80 p-4">
      <TenantSwitcher
        :tenants="store.tenants"
        :selected-tenant-id="store.selectedTenantId"
        :loading="store.isLoading"
        :error="store.error"
        @select="selectTenant"
        @retry="retryLoad"
      />
    </div>
  `,
});

const meta = {
  title: 'Header/TenantSwitcher',
  component: TenantSwitcher,
  tags: ['autodocs'],
  parameters: {
    docs: {
      description: {
        component:
          'ヘッダーのテナント選択。開くと現在地・テナント設定への導線・切り替え・作成が 1 枚に載る。',
      },
    },
  },
} satisfies Meta<typeof TenantSwitcher>;

export default meta;
type Story = StoryObj;

export const MultipleTenants: Story = {
  render: renderWithStore({
    tenants: [primaryTenant, secondaryTenant],
    selectedTenantId: primaryTenant.id,
  }),
  beforeEach: mockMembers(),
  play: async ({ canvasElement }) => {
    const page = within(canvasElement.ownerDocument.body);
    await userEvent.click(page.getByRole('button', { name: /Acme/ }));
    await userEvent.click(await page.findByRole('menuitem', { name: 'Globex' }));
    await expect(within(canvasElement).getByText('Globex')).toBeInTheDocument();
  },
};

/** 設定への導線は、今いるテナントの URL で組む。 */
export const LinksToTenantSettings: Story = {
  render: renderWithStore({
    tenants: [primaryTenant, secondaryTenant],
    selectedTenantId: primaryTenant.id,
  }),
  beforeEach: mockMembers(),
  play: async ({ canvasElement }) => {
    const page = within(canvasElement.ownerDocument.body);
    await userEvent.click(page.getByRole('button', { name: /Acme/ }));

    await expect(page.findByText('3 人のメンバー')).resolves.toBeInTheDocument();
    await expect(page.getByRole('menuitem', { name: 'テナント設定' })).toHaveAttribute(
      'href',
      '/acme/settings',
    );
    await expect(page.getByRole('menuitem', { name: 'メンバー' })).toHaveAttribute(
      'href',
      '/acme/settings/members',
    );
  },
};

export const Loading: Story = {
  render: renderWithStore({ tenants: [], selectedTenantId: null, isLoading: true }),
  beforeEach: mockMembers(),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.getByText('テナントを読み込み中…')).toBeInTheDocument();
    await expect(canvas.getByRole('button')).toBeDisabled();
  },
};

export const NoMemberships: Story = {
  render: renderWithStore({ tenants: [], selectedTenantId: null }),
  beforeEach: mockMembers(),
  play: async ({ canvasElement }) => {
    const page = within(canvasElement.ownerDocument.body);
    await userEvent.click(page.getByRole('button', { name: /所属テナントなし/ }));
    await expect(await page.findByText('所属テナントがありません')).toBeInTheDocument();
    // テナントが決まっていないので、設定への導線は出さない（行き先を組めない）
    await expect(page.queryByRole('menuitem', { name: 'テナント設定' })).not.toBeInTheDocument();
    await expect(page.getByRole('button', { name: 'テナントを作成' })).toBeInTheDocument();
  },
};

export const ApiError: Story = {
  render: renderWithStore({
    tenants: [],
    selectedTenantId: null,
    error: 'テナント一覧を取得できませんでした',
  }),
  beforeEach: mockMembers(),
  play: async ({ canvasElement }) => {
    retry.mockClear();
    const page = within(canvasElement.ownerDocument.body);
    await userEvent.click(page.getByRole('button', { name: /所属テナントなし/ }));
    await userEvent.click(await page.findByText(/再試行/));
    await expect(retry).toHaveBeenCalledOnce();
  },
};

export const SingleTenant: Story = {
  render: renderWithStore({ tenants: [primaryTenant], selectedTenantId: primaryTenant.id }),
  beforeEach: mockMembers(1),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.getByText('Acme')).toBeInTheDocument();
  },
};

export const MissingSelectionFallsBack: Story = {
  name: 'Missing Selection Shows Not Found',
  render: renderWithStore({
    tenants: [primaryTenant, secondaryTenant],
    selectedTenantId: '00000000-0000-4000-8000-000000000099',
  }),
  beforeEach: mockMembers(),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.getByText('指定されたテナントが見つかりません')).toBeInTheDocument();
    await expect(canvas.queryByText('Acme')).not.toBeInTheDocument();
  },
};
