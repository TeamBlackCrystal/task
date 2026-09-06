import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest';
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils';
import { VueQueryPlugin, QueryClient } from '@tanstack/vue-query';
import { createPinia } from 'pinia';
import { h } from 'vue';

vi.mock('@/components/header/AppHeader.vue', () => ({
  default: { name: 'AppHeader', template: '<div />' },
  // Vue Test Utils は遅延コンポーネントのモジュールにも組み込み型の印を問い合わせる。
  __isKeepAlive: false,
  __isTeleport: false,
}));

import Layout from '../+Layout.vue';

enableAutoUnmount(afterEach);

const mockUser = {
  id: '00000000-0000-0000-0000-000000000001',
  email: 'test@example.com',
  username: 'testuser',
  email_verified: true,
  is_admin: false,
  is_suspended: false,
  totp_enabled: false,
};

// 認証ガードが有効なページでは /v1/auth/me が飛ぶ。応答は返さず未解決のままにして
// 「解決を待っている」状態を再現する
beforeEach(() => {
  vi.stubGlobal(
    'fetch',
    vi.fn(() => new Promise<Response>(() => {})),
  );
});

afterEach(() => {
  vi.unstubAllGlobals();
});

// サインイン前に開くページは認証ガードを通さず、そのまま中身を描画する。
// ここから漏れると /signin へリダイレクトされ、メール確認・パスワード再設定が完了できない。
const PRE_AUTH_PATHS = ['/signin', '/signup', '/auth/reset-password', '/verify-email'];

function mountLayout(urlPathname: string) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return mount(Layout, {
    slots: { default: () => h('div', 'ページ本体') },
    global: {
      plugins: [createPinia(), [VueQueryPlugin, { queryClient }]],
      provide: { 'vike-vue:usePageContext': { urlPathname, urlParsed: { search: {} } } },
      // vike のランタイム前提のコンポーネントは単体マウントできない
      stubs: { ClientOnly: true, AppHeader: true, AppSidebar: true, AppSidebarSkeleton: true },
    },
  });
}

describe('+Layout', () => {
  it.each(PRE_AUTH_PATHS)('%s は認証ガードを通さずページを描画する', async (path) => {
    const wrapper = mountLayout(path);
    await flushPromises();

    expect(wrapper.text()).toContain('ページ本体');
    expect(wrapper.text()).not.toContain('読み込み中…');
  });

  it('通常のページは認証の解決を待つ', async () => {
    const wrapper = mountLayout('/my-tasks');
    await flushPromises();

    expect(wrapper.text()).not.toContain('ページ本体');
  });

  it('アプリヘッダーを固定し、本文だけをスクロールさせる', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(
        async () =>
          new Response(JSON.stringify(mockUser), {
            status: 200,
            headers: { 'Content-Type': 'application/json' },
          }),
      ),
    );

    const wrapper = mountLayout('/my-tasks');
    await flushPromises();

    const shell = wrapper.get('[data-slot="sidebar-wrapper"]');
    expect(shell.classes()).toEqual(expect.arrayContaining(['h-svh', 'overflow-hidden']));

    const scrollContainer = wrapper.get('[data-slot="sidebar-inset"]');
    expect(scrollContainer.classes()).toEqual(
      expect.arrayContaining(['min-h-0', 'min-w-0', 'overflow-y-auto']),
    );
    expect(scrollContainer.element.contains(wrapper.get('app-header-stub').element)).toBe(false);
  });
});
