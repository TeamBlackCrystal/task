import type { Meta, StoryObj } from '@storybook/vue3-vite';
import { createPinia, setActivePinia } from 'pinia';
import { fn } from 'storybook/test';

import NavUser from '@/components/header/NavUser.vue';
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

const acme = tenant('00000000-0000-4000-8000-000000000010', 'Acme Inc', 'acme');
const globex = tenant('00000000-0000-4000-8000-000000000020', 'Globex', 'globex');

const user = { name: 'yupix', email: 'm@example.com', avatar: '' };

/**
 * AppHeader は Pinia と pageContext から値を取るので、ストーリーでは同じ構成を
 * 手で組んで見た目だけを確認する（テナントは左、アカウントは右）。
 */
const renderHeader =
  (options: { tenants?: Tenant[]; selectedTenantId?: string | null } = {}) =>
  () => ({
    components: { NavUser, TenantSwitcher },
    setup() {
      setActivePinia(createPinia());
      const store = useTenantStore();
      store.$patch({
        tenants: options.tenants ?? [acme, globex],
        selectedTenantId: options.selectedTenantId ?? acme.id,
        isLoading: false,
        error: null,
      });
      return { store, user, logout: fn(), selectTenant: (t: Tenant) => store.selectTenant(t) };
    },
    template: `
      <header class="flex h-12 shrink-0 items-center gap-2 border-b bg-sidebar px-3">
        <TenantSwitcher
          :tenants="store.tenants"
          :selected-tenant-id="store.selectedTenantId"
          :loading="store.isLoading"
          :error="store.error"
          @select="selectTenant"
        />
        <div class="ml-auto flex items-center gap-1">
          <NavUser :user="user" :on-logout="logout" />
        </div>
      </header>
    `,
  });

const meta = {
  title: 'Header/AppHeader',
  component: TenantSwitcher,
  parameters: { layout: 'fullscreen' },
} satisfies Meta<typeof TenantSwitcher>;

export default meta;

type Story = StoryObj;

export const Default: Story = { render: renderHeader() };

export const NoTenants: Story = {
  render: renderHeader({ tenants: [], selectedTenantId: null }),
};

/** ヘッダーとサイドバーの重なりを見るための並び。 */
export const WithSidebar: Story = {
  render: () => ({
    components: { NavUser, TenantSwitcher, SidebarProvider },
    setup() {
      setActivePinia(createPinia());
      const store = useTenantStore();
      store.$patch({
        tenants: [acme, globex],
        selectedTenantId: acme.id,
        isLoading: false,
        error: null,
      });
      return { store, user, logout: fn() };
    },
    template: `
      <SidebarProvider class="min-h-0 h-svh flex-col">
        <header class="flex h-12 shrink-0 items-center gap-2 border-b bg-sidebar px-3">
          <TenantSwitcher
            :tenants="store.tenants"
            :selected-tenant-id="store.selectedTenantId"
            :loading="store.isLoading"
            :error="store.error"
          />
          <div class="ml-auto flex items-center gap-1">
            <NavUser :user="user" :on-logout="logout" />
          </div>
        </header>
        <div class="flex min-h-0 w-full flex-1">
          <div class="w-64 shrink-0 border-r bg-sidebar p-3 text-sm text-muted-foreground">
            サイドバー（ナビ）
          </div>
          <div class="flex-1 p-4 text-sm text-muted-foreground">本文</div>
        </div>
      </SidebarProvider>
    `,
  }),
};
