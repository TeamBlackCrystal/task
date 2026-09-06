import { describe, it, expect, afterEach, vi } from 'vitest';
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils';
import { VueQueryPlugin, QueryClient } from '@tanstack/vue-query';
import SignInForm from '../SignInForm.vue';
import { PASSWORD_CHANGED_NOTICE } from '@/lib/one-time-notice';

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
  // 通知の印はタブに残るので、テスト間で持ち越さない
  window.sessionStorage.clear();
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

    // form に novalidate がないと、type="email" のネイティブ制約検証が submit を
    // 先に止めて handleSubmit が呼ばれず、下のエラーが出なくなる。
    // happy-dom は制約検証をしないので、この属性の検査が唯一の回帰ガードになる
    expect(wrapper.find('form').attributes('novalidate')).toBeDefined();

    await wrapper.find('button[type="submit"]').trigger('click');
    await flushPromises();

    expect(document.body.textContent).toContain('メールアドレスの形式が正しくありません');
    const loginCalls = fetchMock.mock.calls.filter(([req]) =>
      (typeof req === 'string' ? req : req.url).includes('/v1/auth/login'),
    );
    expect(loginCalls).toHaveLength(0);
  });

  it('パスワードのエラーを出した後に直せば、フォーカスを外さなくても 1 回目の送信が通る', async () => {
    const fetchMock = stubLogin(200, {});
    const wrapper = await mountForm();

    const password = wrapper.find('#password');
    await wrapper.find('#email').setValue('test@example.com');
    await password.setValue('pass');
    await password.trigger('blur');
    await flushPromises();
    expect(document.body.textContent).toContain('8文字以上で入力してください');

    // blur せずに直してそのまま送信（フィールド内で Enter を押した場合の経路）
    await password.setValue('devpass123');
    await wrapper.find('form').trigger('submit');
    await flushPromises();

    const loginCalls = fetchMock.mock.calls.filter(([req]) =>
      (typeof req === 'string' ? req : req.url).includes('/v1/auth/login'),
    );
    expect(loginCalls).toHaveLength(1);
    expect(document.body.textContent).not.toContain('8文字以上で入力してください');
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

describe('SignInForm のパスワード変更の案内', () => {
  const NOTICE = 'すべてのセッションとパーソナルアクセストークンが失効したため';

  it('設定画面が置いた印があるときだけ出す', async () => {
    stubLogin(200, {});
    window.sessionStorage.setItem(PASSWORD_CHANGED_NOTICE, '1');
    const wrapper = await mountForm();

    expect(wrapper.text()).toContain(NOTICE);
  });

  // URL を開かせるだけでこの案内を出せると、変えていない人にパスワードが変わったと
  // 思わせられる。根拠は本人のタブに置いた印だけにする
  it('印が無ければ URL に印を付けても出さない', async () => {
    stubLogin(200, {});
    window.history.replaceState(null, '', '/signin?password_changed=1');
    const wrapper = await mountForm();

    expect(wrapper.text()).not.toContain(NOTICE);
  });

  it('印は一度きりで、開き直しても出ない', async () => {
    stubLogin(200, {});
    window.sessionStorage.setItem(PASSWORD_CHANGED_NOTICE, '1');
    await mountForm();

    const second = await mountForm();

    expect(second.text()).not.toContain(NOTICE);
  });
});
