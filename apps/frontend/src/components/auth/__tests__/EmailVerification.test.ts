import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils';
import { VueQueryPlugin, QueryClient } from '@tanstack/vue-query';

const { verifyMutateAsync } = vi.hoisted(() => ({
  verifyMutateAsync: vi.fn(),
}));

vi.mock('@/lib/api-vue-query', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api-vue-query')>();
  return {
    ...actual,
    useVerifyEmailMutation: () => ({ mutateAsync: verifyMutateAsync }),
  };
});

import EmailVerification from '../EmailVerification.vue';

enableAutoUnmount(afterEach);

beforeEach(() => {
  verifyMutateAsync.mockReset();
});

function mountView(search: Record<string, string>) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return mount(EmailVerification, {
    global: {
      plugins: [[VueQueryPlugin, { queryClient }]],
      provide: { 'vike-vue:usePageContext': { urlParsed: { search } } },
    },
  });
}

describe('EmailVerification', () => {
  it('トークンを API に渡し、成功したら確認完了を表示する', async () => {
    verifyMutateAsync.mockResolvedValue('Email verified');

    const wrapper = mountView({ token: 'tok-123' });
    await flushPromises();

    expect(verifyMutateAsync).toHaveBeenCalledWith({
      body: { token: 'tok-123' },
      parseAs: 'text',
    });
    expect(wrapper.text()).toContain('メールアドレスを確認しました');
  });

  it('API が失敗したら無効なリンクとして表示する', async () => {
    verifyMutateAsync.mockRejectedValue(new Error('invalid token'));

    const wrapper = mountView({ token: 'expired' });
    await flushPromises();

    expect(wrapper.text()).toContain('リンクが無効です');
  });

  it('トークンが無いときは API を呼ばない', async () => {
    const wrapper = mountView({});
    await flushPromises();

    expect(verifyMutateAsync).not.toHaveBeenCalled();
    expect(wrapper.text()).toContain('リンクが無効です');
  });
});
