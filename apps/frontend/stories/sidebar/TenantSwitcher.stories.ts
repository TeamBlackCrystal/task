import type { Meta, StoryObj } from '@storybook/vue3-vite';
import { createPinia, setActivePinia } from 'pinia';
import { expect, fn, userEvent, within } from 'storybook/test';
import TenantSwitcher from '@/components/header/TenantSwitcher.vue';
import { SidebarProvider } from '@/components/ui/sidebar';
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

type TenantState = {
  tenants: Tenant[];
  selectedTenantId: string | null;
  isLoading?: boolean;
  error?: string | null;
};

const renderWithStore = (state: TenantState) => () => ({
  components: { SidebarProvider, TenantSwitcher },
  setup() {
    const pinia = createPinia();
    setActivePinia(pinia);
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
    <SidebarProvider class="min-h-0 w-80 p-4">
      <TenantSwitcher
        :tenants="store.tenants"
        :selected-tenant-id="store.selectedTenantId"
        :loading="store.isLoading"
        :error="store.error"
        @select="selectTenant"
        @retry="retryLoad"
      />
    </SidebarProvider>
  `,
});

const meta = {
  title: 'Sidebar/TenantSwitcher',
  component: TenantSwitcher,
  tags: ['autodocs'],
} satisfies Meta<typeof TenantSwitcher>;

export default meta;
type Story = StoryObj;

export const MultipleTenants: Story = {
  render: renderWithStore({
    tenants: [primaryTenant, secondaryTenant],
    selectedTenantId: primaryTenant.id,
  }),
  play: async ({ canvasElement }) => {
    const page = within(canvasElement.ownerDocument.body);
    await userEvent.click(page.getByRole('button', { name: /Acme/ }));
    await userEvent.click(await page.findByText('Globex'));
    await expect(within(canvasElement).getByText('Globex')).toBeInTheDocument();
  },
};

export const Loading: Story = {
  render: renderWithStore({ tenants: [], selectedTenantId: null, isLoading: true }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.getByText('テナントを読み込み中…')).toBeInTheDocument();
    await expect(canvas.getByRole('button')).toBeDisabled();
  },
};

export const NoMemberships: Story = {
  render: renderWithStore({ tenants: [], selectedTenantId: null }),
  play: async ({ canvasElement }) => {
    const page = within(canvasElement.ownerDocument.body);
    await userEvent.click(page.getByRole('button', { name: /所属テナントなし/ }));
    await expect(await page.findByText('所属テナントがありません')).toBeInTheDocument();
    await expect(page.getByText('Add tenant')).toBeInTheDocument();
  },
};

export const ApiError: Story = {
  render: renderWithStore({
    tenants: [],
    selectedTenantId: null,
    error: 'テナント一覧を取得できませんでした',
  }),
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
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.getByText('指定されたテナントが見つかりません')).toBeInTheDocument();
    await expect(canvas.queryByText('Acme')).not.toBeInTheDocument();
  },
};
