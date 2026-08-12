import { describe, it, expect, afterEach, vi } from 'vitest';
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils';
import { VueQueryPlugin, QueryClient } from '@tanstack/vue-query';
import SignInForm from '../SignInForm.vue';

const jsonResponse = (data: unknown, status = 200) =>
  new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });

function stubLogin(status: number, body: unknown) {
  const fetchMock = vi.fn(async (req: Request | string) => {
    const url = typeof req === 'string' ? req : req.url;
    const pathname = new URL(url, 'http://localhost').pathname;
    if (pathname.endsWith('/v1/auth/login')) {
      return jsonResponse(body, status);
    }
    if (pathname.endsWith('/v1/auth/oauth/providers')) {
      return jsonResponse({ providers: [] });
    }
    return jsonResponse({ message: 'not-found' }, 404);
  });
  vi.stubGlobal('fetch', fetchMock);
  return fetchMock;
}

async function mountAndSubmit() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const wrapper = mount(SignInForm, {
    global: { plugins: [[VueQueryPlugin, { queryClient }]] },
    attachTo: document.body,
  });
  await flushPromises();
  await wrapper.find('#email').setValue('test@example.com');
  await wrapper.find('#password').setValue('devpass123');
  await wrapper.find('form').trigger('submit');
  await flushPromises();
  return wrapper;
}

enableAutoUnmount(afterEach);

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('SignInForm', () => {
  it('email-not-verified の 403 ではメール未認証画面を表示する', async () => {
    stubLogin(403, { message: 'email-not-verified' });
    await mountAndSubmit();

    expect(document.body.textContent).toContain('メールアドレスを確認してください');
  });

  it('メール未認証以外の 403（CSRF 拒否など）では未認証画面を出さずエラーを表示する', async () => {
    stubLogin(403, { message: 'forbidden' });
    await mountAndSubmit();

    expect(document.body.textContent).not.toContain('メールアドレスを確認してください');
    expect(document.body.textContent).toContain(
      'メールアドレスまたはパスワードが正しくありません。',
    );
  });

  it('401 ではエラーメッセージを表示する', async () => {
    stubLogin(401, { message: 'invalid-credentials' });
    await mountAndSubmit();

    expect(document.body.textContent).toContain(
      'メールアドレスまたはパスワードが正しくありません。',
    );
  });
});
