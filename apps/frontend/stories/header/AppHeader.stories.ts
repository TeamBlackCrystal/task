import type { Meta, StoryObj } from '@storybook/vue3-vite';
import { QueryClient, VUE_QUERY_CLIENT } from '@tanstack/vue-query';
import { createPinia, setActivePinia } from 'pinia';
import { fn, expect, within } from 'storybook/test';
import { provide } from 'vue';

import AppHeader from '@/components/header/AppHeader.vue';
import NavUser from '@/components/header/NavUser.vue';
import TenantSwitcher from '@/components/header/TenantSwitcher.vue';
import AppSidebar from '@/components/sidebar/AppSidebar.vue';
import { useAuthStore } from '@/stores/auth';
import { SidebarInset, SidebarProvider } from '@/components/ui/sidebar';
import type { Tenant } from '@/stores/tenant';
import { useTenantStore } from '@/stores/tenant';

const PAGE_CONTEXT_KEY = 'vike-vue:usePageContext';

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

const authUser = {
  id: ownerId,
  email: user.email,
  username: user.name,
  email_verified: true,
  is_admin: false,
  is_suspended: false,
  totp_enabled: false,
  avatar_url: null,
  bio: null,
};

const layoutPageContext = {
  urlPathname: '/acme/my-tasks',
  routeParams: { tenant: 'acme' },
};

const jsonResponse = (data: unknown) =>
  new Response(JSON.stringify(data), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  });

function mockLayoutFetch() {
  const original = globalThis.fetch;
  globalThis.fetch = fn().mockImplementation(async (request: Request | string) => {
    const url = typeof request === 'string' ? request : request.url;
    const pathname = new URL(url, 'http://localhost').pathname;

    if (pathname === '/v1/auth/me') return jsonResponse(authUser);
    if (pathname === '/v1/tenants') return jsonResponse([acme, globex]);
    if (pathname.endsWith(`/tenants/${acme.id}/projects`)) return jsonResponse([]);
    return jsonResponse([]);
  });

  return () => {
    globalThis.fetch = original;
  };
}

function realLayoutDecorator() {
  return () => ({
    setup() {
      setActivePinia(createPinia());
      const queryClient = new QueryClient({
        defaultOptions: {
          queries: { retry: false, gcTime: 0, staleTime: 0 },
          mutations: { retry: false },
        },
      });
      provide(VUE_QUERY_CLIENT, queryClient);
      provide(PAGE_CONTEXT_KEY, layoutPageContext);

      useAuthStore().setUser(authUser);
    },
    template: '<story />',
  });
}

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
  decorators: [realLayoutDecorator()],
  beforeEach: mockLayoutFetch,
  render: () => ({
    components: { AppHeader, AppSidebar, SidebarInset, SidebarProvider },
    template: `
      <SidebarProvider class="min-h-0 h-svh flex-col">
        <AppHeader />
        <div class="flex min-h-0 w-full flex-1">
          <AppSidebar desktop-top-offset="3rem" />
          <SidebarInset>
            <div class="flex-1 p-4 text-sm text-muted-foreground">本文</div>
          </SidebarInset>
        </div>
      </SidebarProvider>
    `,
  }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.getByRole('banner')).toBeInTheDocument();

    const desktopSidebar = canvasElement.querySelector<HTMLElement>(
      '[data-slot="sidebar"] > [style]',
    );
    if (!desktopSidebar) throw new Error('desktop Sidebar was not rendered');
    await expect(desktopSidebar.style.top).toBe('3rem');
    await expect(desktopSidebar.style.height).toContain('100svh');
    await expect(desktopSidebar.style.height).toContain('3rem');
  },
};
