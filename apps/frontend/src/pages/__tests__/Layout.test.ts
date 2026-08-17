import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest';
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils';
import { VueQueryPlugin, QueryClient } from '@tanstack/vue-query';
import { createPinia } from 'pinia';
import { h } from 'vue';

import Layout from '../+Layout.vue';

enableAutoUnmount(afterEach);

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
      stubs: { ClientOnly: true, AppSidebar: true, AppSidebarSkeleton: true },
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
});
