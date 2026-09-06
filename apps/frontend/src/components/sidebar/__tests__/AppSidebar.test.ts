import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { computed, defineComponent, h, ref } from 'vue';
import { enableAutoUnmount, mount } from '@vue/test-utils';

const { navigateSpy, projectsControl, tenantStoreStub } = vi.hoisted(() => ({
  navigateSpy: vi.fn(),
  projectsControl: { data: [] as unknown[], isLoading: false, isError: false },
  tenantStoreStub: {
    tenants: [] as unknown[],
    selectedTenantId: null as string | null,
    isLoading: false,
    error: null as string | null,
    loadTenants: vi.fn(),
    selectTenant: vi.fn(),
  },
}));

vi.mock('vike/client/router', () => ({ navigate: navigateSpy }));

vi.mock('vike-vue/usePageContext', () => ({
  usePageContext: () => ({ routeParams: { tenant: 'acme' }, urlPathname: '/acme/my-tasks' }),
}));

vi.mock('@/composables/useRouteAlignedTenantId', () => ({
  useRouteAlignedTenantId: () => computed(() => 'tenant-1'),
}));

vi.mock('@/stores/tenant', () => ({ useTenantStore: () => tenantStoreStub }));

vi.mock('@/lib/api-vue-query', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api-vue-query')>();
  return {
    ...actual,
    useProjectsQuery: () => ({
      data: computed(() => projectsControl.data),
      isLoading: computed(() => projectsControl.isLoading),
      isError: computed(() => projectsControl.isError),
      refetch: vi.fn(),
    }),
  };
});

import { TooltipProvider } from 'reka-ui';

import AppSidebar from '../AppSidebar.vue';
import { provideSidebarContext } from '../../ui/sidebar/utils';

enableAutoUnmount(afterEach);

/**
 * モバイルのサイドバーを開いた状態で AppSidebar を組む。
 *
 * `SidebarProvider` は `useMediaQuery` で isMobile を決めるので、matchMedia を
 * 細工するより context を直接 provide したほうが状態を確実に固定できる。
 */
function mountSidebar(setOpenMobile: (value: boolean) => void) {
  const Host = defineComponent({
    setup() {
      provideSidebarContext({
        state: computed(() => 'expanded' as const),
        open: ref(true),
        setOpen: vi.fn(),
        isMobile: ref(true),
        openMobile: ref(true),
        setOpenMobile,
        toggleSidebar: vi.fn(),
      });
      // `collapsible: 'none'` で素の div 描画にする。isMobile のままだと Sheet
      // （portal）が挟まり jsdom で組めないが、閉じる処理は context の isMobile を
      // 見るので、この指定でも検証したい経路は変わらない。
      // TooltipProvider は本番では SidebarProvider が用意している（サイドバーの
      // ボタンが Tooltip を使う）。
      return () =>
        h(TooltipProvider, { delayDuration: 0 }, () => h(AppSidebar, { collapsible: 'none' }));
    },
  });
  return mount(Host, { attachTo: document.body });
}

function findLink(href: string) {
  const link = document.body.querySelector<HTMLAnchorElement>(`a[href="${href}"]`);
  if (!link) throw new Error(`link "${href}" not found`);
  return link;
}

function findButton(label: string) {
  const button = [...document.body.querySelectorAll('button')].find(
    (el) => el.getAttribute('title') === label || el.textContent?.trim() === label,
  );
  if (!button) throw new Error(`button "${label}" not found`);
  return button as HTMLButtonElement;
}

beforeEach(() => {
  navigateSpy.mockReset();
  projectsControl.data = [];
  projectsControl.isLoading = false;
  projectsControl.isError = false;
  tenantStoreStub.tenants = [{ id: 'tenant-1', display_id: 'acme', name: 'Acme' }];
});

/**
 * 「プロジェクトを作成」は <button> から navigate を呼ぶ。リンクを踏まないので
 * SidebarContent のイベント委譲（closest('a')）では拾えず、遷移しても作成画面が
 * モバイルのサイドバーに覆われたままになっていた。導線は 2 か所ある。
 */
/**
 * ナビのリンクは SidebarContent に置いたイベント委譲で拾う。効かなくなる現実的な
 * 壊れ方は `@click` の行が消えることではなく、ナビが SidebarContent の外へ移ったり
 * 別のコンポーネントで包まれたりして委譲の下から抜けることなので、純粋関数ではなく
 * 実際のマークアップから辿って確認する。
 */
describe('AppSidebar のナビリンク', () => {
  it('モバイルでナビのリンクを押したらサイドバーを閉じる', () => {
    const setOpenMobile = vi.fn<(value: boolean) => void>();
    mountSidebar(setOpenMobile);

    findLink('/acme/my-tasks').click();

    expect(setOpenMobile).toHaveBeenCalledWith(false);
  });
});

describe('AppSidebar のプロジェクト作成導線', () => {
  it('空状態の作成ボタンで、遷移してサイドバーを閉じる', async () => {
    const setOpenMobile = vi.fn<(value: boolean) => void>();
    mountSidebar(setOpenMobile);

    findButton('プロジェクトを作成').click();
    await Promise.resolve();

    expect(navigateSpy).toHaveBeenCalledWith('/acme/projects/new');
    expect(setOpenMobile).toHaveBeenCalledWith(false);
  });

  it('プロジェクトがある状態の見出し横の作成ボタンでも、遷移してサイドバーを閉じる', async () => {
    projectsControl.data = [
      {
        id: 'project-1',
        key: 'ALPHA',
        name: 'Team Alpha',
        description: '',
        icon_emoji: null,
        icon_url: null,
        is_personal: false,
        personal_owner_id: null,
        tenant_id: 'tenant-1',
      },
    ];
    const setOpenMobile = vi.fn<(value: boolean) => void>();
    mountSidebar(setOpenMobile);

    // 空状態のカードは出ないので、拾えるのは見出し横のアクションだけ
    findButton('プロジェクトを作成').click();
    await Promise.resolve();

    expect(navigateSpy).toHaveBeenCalledWith('/acme/projects/new');
    expect(setOpenMobile).toHaveBeenCalledWith(false);
  });
});
