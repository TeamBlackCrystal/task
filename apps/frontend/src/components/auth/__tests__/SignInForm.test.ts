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

async function mountForm() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const wrapper = mount(SignInForm, {
    global: { plugins: [[VueQueryPlugin, { queryClient }]] },
    attachTo: document.body,
  });
  await flushPromises();
  return wrapper;
}

async function mountAndSubmit() {
  const wrapper = await mountForm();
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
  it('メール形式のエラーは、フォーカスを外さなくても入力を直した時点で消える', async () => {
    stubLogin(200, {});
    const wrapper = await mountForm();

    const email = wrapper.find('#email');
    await email.setValue('not-an-email');
    await email.trigger('blur');
    await flushPromises();
    expect(document.body.textContent).toContain('メールアドレスの形式が正しくありません');

    // blur せずに入力を直すだけでエラーが消える（onChange 検証が効いていること）
    await email.setValue('test@example.com');
    await flushPromises();
    expect(document.body.textContent).not.toContain('メールアドレスの形式が正しくありません');
  });

  it('入力途中（まだフォーカスを外していない）ではメール形式のエラーを出さない', async () => {
    stubLogin(200, {});
    const wrapper = await mountForm();

    await wrapper.find('#email').setValue('tes');
    await flushPromises();

    expect(document.body.textContent).not.toContain('メールアドレスの形式が正しくありません');
  });

  it('一度もフォーカスを外していない不正なメールでも、送信ボタンは押せて理由が表示される', async () => {
    const fetchMock = stubLogin(200, {});
    const wrapper = await mountForm();

    await wrapper.find('#password').setValue('devpass123');
    await wrapper.find('#email').setValue('tes');
    await flushPromises();

    // 入力途中でボタンを無効化しない（無効なボタンは押しても blur せず、行き止まりになる）
    expect(wrapper.find('button[type="submit"]').attributes('disabled')).toBeUndefined();

    await wrapper.find('form').trigger('submit');
    await flushPromises();

    expect(document.body.textContent).toContain('メールアドレスの形式が正しくありません');
    const loginCalls = fetchMock.mock.calls.filter(([req]) =>
      (typeof req === 'string' ? req : req.url).includes('/v1/auth/login'),
    );
    expect(loginCalls).toHaveLength(0);
  });

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
