import { defineComponent, h, inject, provide, ref, type Ref } from 'vue';
import { QueryClient, VUE_QUERY_CLIENT } from '@tanstack/vue-query';
import { afterEach, describe, expect, it } from 'vitest';
import { enableAutoUnmount, mount } from '@vue/test-utils';
import type { Tenant } from '@/stores/tenant';
import SidebarProvider from '../../ui/sidebar/SidebarProvider.vue';
import TenantSwitcher from '../TenantSwitcher.vue';

enableAutoUnmount(afterEach);

const tenants: Tenant[] = [
  {
    id: 'tenant-1',
    display_id: 'alpha',
    name: 'Alpha',
    description: '',
    icon_url: 'https://example.com/alpha.png',
    owner_id: 'owner-1',
    require_2fa: false,
  },
  {
    id: 'tenant-2',
    display_id: 'beta',
    name: 'Beta',
    description: '',
    icon_url: '',
    owner_id: 'owner-1',
    require_2fa: false,
  },
];

const DROPDOWN_OPEN = Symbol('dropdown-open');

const DropdownMenu = defineComponent({
  name: 'DropdownMenu',
  setup(_, { slots }) {
    const open = ref(false);
    provide(DROPDOWN_OPEN, open);
    return () => slots.default?.();
  },
});

const DropdownMenuTrigger = defineComponent({
  name: 'DropdownMenuTrigger',
  inheritAttrs: false,
  setup(_, { slots, attrs }) {
    const open = inject<Ref<boolean>>(DROPDOWN_OPEN)!;
    return () =>
      h(
        'button',
        {
          ...attrs,
          type: 'button',
          'data-testid': 'tenant-switcher-trigger',
          onClick: () => {
            open.value = !open.value;
          },
        },
        slots.default?.(),
      );
  },
});

const DropdownMenuContent = defineComponent({
  name: 'DropdownMenuContent',
  setup(_, { slots }) {
    const open = inject<Ref<boolean>>(DROPDOWN_OPEN)!;
    return () =>
      open.value ? h('div', { 'data-testid': 'tenant-switcher-menu' }, slots.default?.()) : null;
  },
});

const PassThrough = defineComponent({ template: '<div><slot /></div>' });
const Item = defineComponent({
  inheritAttrs: false,
  template: '<button v-bind="$attrs"><slot /></button>',
});
const ButtonStub = defineComponent({
  inheritAttrs: false,
  template: '<button v-bind="$attrs"><slot /></button>',
});

function mountSwitcher(props: {
  tenants: Tenant[];
  selectedTenantId: string | null;
  loading?: boolean;
  error?: string | null;
}) {
  return mount(
    {
      components: { SidebarProvider, TenantSwitcher },
      template: '<SidebarProvider><TenantSwitcher v-bind="props" /></SidebarProvider>',
      setup: () => {
        provide(
          VUE_QUERY_CLIENT,
          new QueryClient({
            defaultOptions: { queries: { retry: false, gcTime: 0, staleTime: 0 } },
          }),
        );
        return { props };
      },
    },
    {
      global: {
        stubs: {
          DropdownMenu,
          DropdownMenuTrigger,
          DropdownMenuContent,
          DropdownMenuLabel: PassThrough,
          DropdownMenuSeparator: PassThrough,
          DropdownMenuItem: Item,
          SidebarMenu: PassThrough,
          SidebarMenuItem: PassThrough,
          SidebarMenuButton: ButtonStub,
          CreateTenantDialog: true,
        },
      },
    },
  );
}

describe('TenantSwitcher', () => {
  it('opens the tenant menu and emits the selected tenant', async () => {
    const wrapper = mountSwitcher({ tenants, selectedTenantId: 'tenant-1' });

    expect(wrapper.text()).toContain('Alpha');
    expect(wrapper.text()).not.toContain('Beta');

    await wrapper.get('[data-testid="tenant-switcher-trigger"]').trigger('click');
    const menu = wrapper.get('[data-testid="tenant-switcher-menu"]');
    expect(menu.text()).toContain('Beta');

    const beta = menu.findAll('button').find((button) => button.text().includes('Beta'))!;
    await beta.trigger('click');

    expect(wrapper.findComponent(TenantSwitcher).emitted('select')).toEqual([[tenants[1]]]);
  });

  it('shows the empty state when the user has no tenant memberships', async () => {
    const wrapper = mountSwitcher({ tenants: [], selectedTenantId: null });

    // ヘッダーのトリガーは 1 行なので、状態は主文で伝える
    expect(wrapper.text()).toContain('所属テナントなし');

    await wrapper.get('[data-testid="tenant-switcher-trigger"]').trigger('click');
    expect(wrapper.get('[data-testid="tenant-switcher-menu"]').text()).toContain(
      '所属テナントがありません',
    );
  });

  it('shows a disabled loading state', () => {
    const wrapper = mountSwitcher({ tenants: [], selectedTenantId: null, loading: true });

    expect(wrapper.text()).toContain('テナントを読み込み中…');
    expect(wrapper.find('button[disabled]').exists()).toBe(true);
  });

  it('shows an API error and emits retry when it is selected', async () => {
    const wrapper = mountSwitcher({
      tenants: [],
      selectedTenantId: null,
      error: 'テナント一覧を取得できませんでした',
    });

    await wrapper.get('[data-testid="tenant-switcher-trigger"]').trigger('click');

    const retry = wrapper
      .get('[data-testid="tenant-switcher-menu"]')
      .findAll('button')
      .find((button) => button.text().includes('テナント一覧を取得できませんでした'))!;
    expect(retry.text()).toContain('再試行');
    await retry.trigger('click');

    expect(wrapper.findComponent(TenantSwitcher).emitted('retry')).toEqual([[]]);
  });

  it('shows not-found instead of silently displaying the first tenant', () => {
    const wrapper = mountSwitcher({ tenants, selectedTenantId: null });

    expect(wrapper.text()).toContain('指定されたテナントが見つかりません');
  });
});
