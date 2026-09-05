import { describe, it, expect, afterEach, vi } from 'vitest';
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils';
import { VueQueryPlugin, QueryClient } from '@tanstack/vue-query';
import AuthMethodsSection from '../AuthMethodsSection.vue';
import type { components } from '@/generated/api';

const USER_ID = '00000000-0000-0000-0000-000000000001';
const INSTANCE_URL = 'https://gitlab.example.com';

function user(hasPassword: boolean): components['schemas']['UserResponse'] {
  return {
    id: USER_ID,
    username: 'tester',
    email: 'tester@example.com',
    email_verified: true,
    is_admin: false,
    is_suspended: false,
    totp_enabled: false,
    has_password: hasPassword,
    bio: null,
    avatar_url: null,
  };
}

type Connection = components['schemas']['OAuthConnectionItem'];
type Provider = components['schemas']['OAuthProviderItem'];

type MockState = {
  connections: Connection[];
  providers: Provider[];
  passkeyCount?: number;
  /** DELETE /oauth/connections/{provider} の返り値。未指定なら 204。 */
  disconnectError?: { status: number; message: string };
  /** POST /password と /password/change の返り値。未指定なら成功。 */
  passwordError?: { status: number; message: string };
};

const jsonResponse = (data: unknown, status = 200) =>
  new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });

function stubFetch(state: MockState) {
  const deleted: string[] = [];
  const passwordBodies: unknown[] = [];

  const fetchMock = vi.fn(async (req: Request | string, init?: RequestInit) => {
    const rawUrl = typeof req === 'string' ? req : req.url;
    const method = typeof req === 'string' ? (init?.method ?? 'GET') : req.method;
    const url = new URL(rawUrl, 'http://localhost');
    const pathname = url.pathname;

    // 強度判定はサーバー側 API。debounce 越しに飛んでも落ちないようにしておく。
    if (pathname.endsWith('/internal/password-strength')) {
      return jsonResponse({ strength: 'high' });
    }
    if (method === 'GET' && pathname.endsWith('/v1/auth/oauth/connections')) {
      return jsonResponse({ connections: state.connections });
    }
    if (method === 'GET' && pathname.endsWith('/v1/auth/oauth/providers')) {
      return jsonResponse({ providers: state.providers });
    }
    if (method === 'GET' && pathname.endsWith('/v1/auth/passkeys')) {
      return jsonResponse({
        passkeys: Array.from({ length: state.passkeyCount ?? 0 }, (_, i) => ({
          id: `passkey-${i}`,
          name: `key ${i}`,
          created_at: '2026-09-01T00:00:00Z',
          last_used_at: null,
        })),
      });
    }
    if (method === 'DELETE' && pathname.includes('/v1/auth/oauth/connections/')) {
      deleted.push(`${pathname}${url.search}`);
      if (state.disconnectError) {
        return jsonResponse(
          { message: state.disconnectError.message },
          state.disconnectError.status,
        );
      }
      const provider = pathname.split('/').pop();
      state.connections = state.connections.filter((c) => c.provider !== provider);
      return new Response(null, { status: 204 });
    }
    if (method === 'POST' && pathname.endsWith('/v1/auth/password')) {
      passwordBodies.push(typeof req === 'string' ? init?.body : await req.clone().json());
      if (state.passwordError) {
        return jsonResponse({ message: state.passwordError.message }, state.passwordError.status);
      }
      return new Response(null, { status: 204 });
    }
    if (method === 'POST' && pathname.endsWith('/v1/auth/password/change')) {
      passwordBodies.push(typeof req === 'string' ? init?.body : await req.clone().json());
      if (state.passwordError) {
        return jsonResponse({ message: state.passwordError.message }, state.passwordError.status);
      }
      return jsonResponse({ message: 'ok' });
    }
    return jsonResponse({ message: 'not-found' }, 404);
  });

  vi.stubGlobal('fetch', fetchMock);
  return { deleted, passwordBodies };
}

function mountSection(hasPassword = true) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return mount(AuthMethodsSection, {
    props: { user: user(hasPassword) },
    global: { plugins: [[VueQueryPlugin, { queryClient }]] },
    attachTo: document.body,
  });
}

function bodyButton(label: string) {
  return [...document.body.querySelectorAll('button')].find((b) => b.textContent?.trim() === label);
}

function clickBodyButton(label: string) {
  const button = bodyButton(label);
  if (!button) throw new Error(`button "${label}" not found`);
  button.click();
}

function connection(overrides: Partial<Connection> = {}): Connection {
  return {
    provider: 'github',
    provider_email: 'tester@example.com',
    instance_url: null,
    connected_at: '2026-09-01T10:00:00Z',
    ...overrides,
  };
}

enableAutoUnmount(afterEach);

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('AuthMethodsSection のパスワード', () => {
  it('設定済みならメールアドレスと変更ボタンを出す', async () => {
    stubFetch({ connections: [], providers: [] });
    mountSection(true);
    await flushPromises();

    expect(document.body.textContent).toContain('設定済み');
    expect(document.body.textContent).toContain('tester@example.com');
    expect(bodyButton('パスワードを変更')).toBeTruthy();
    expect(bodyButton('パスワードを設定')).toBeUndefined();
  });

  it('未設定なら OAuth のみである旨と設定ボタンを出す', async () => {
    stubFetch({ connections: [connection()], providers: [] });
    mountSection(false);
    await flushPromises();

    expect(document.body.textContent).toContain('未設定');
    expect(document.body.textContent).toContain('OAuth 連携のみでサインインしています');
    expect(bodyButton('パスワードを設定')).toBeTruthy();
  });

  it('初回設定では現在のパスワード欄を出さず、設定すると通知を出す', async () => {
    const { passwordBodies } = stubFetch({ connections: [connection()], providers: [] });
    const wrapper = mountSection(false);
    await flushPromises();

    clickBodyButton('パスワードを設定');
    await flushPromises();
    expect(document.body.querySelector('#password-current')).toBeNull();

    await wrapper.find('#password-next').setValue('NewPassword123');
    await wrapper.find('#password-confirm').setValue('NewPassword123');
    await wrapper.find('form').trigger('submit');
    await flushPromises();

    expect(passwordBodies).toEqual([{ password: 'NewPassword123' }]);
    expect(document.body.textContent).toContain('パスワードを設定しました。');
  });

  it('確認が一致しなければ送信しない', async () => {
    const { passwordBodies } = stubFetch({ connections: [], providers: [] });
    const wrapper = mountSection(true);
    await flushPromises();

    clickBodyButton('パスワードを変更');
    await flushPromises();

    await wrapper.find('#password-current').setValue('OldPassword123');
    await wrapper.find('#password-next').setValue('NewPassword123');
    await wrapper.find('#password-confirm').setValue('NewPassword124');
    await wrapper.find('form').trigger('submit');
    await flushPromises();

    expect(passwordBodies).toEqual([]);
    expect(document.body.textContent).toContain('パスワードが一致しません。');
  });

  it('現在のパスワードが違えばサーバーの拒否を伝える', async () => {
    stubFetch({
      connections: [],
      providers: [],
      passwordError: { status: 400, message: 'invalid-current-password' },
    });
    const wrapper = mountSection(true);
    await flushPromises();

    clickBodyButton('パスワードを変更');
    await flushPromises();

    await wrapper.find('#password-current').setValue('WrongPassword123');
    await wrapper.find('#password-next').setValue('NewPassword123');
    await wrapper.find('#password-confirm').setValue('NewPassword123');
    await wrapper.find('form').trigger('submit');
    await flushPromises();

    expect(document.body.textContent).toContain('現在のパスワードが違います。');
  });

  /** 変更でセッションと PAT が失効するので、そのままの状態で留まらせない。 */
  it('変更に成功したらサインイン画面へフルページ遷移する', async () => {
    stubFetch({ connections: [], providers: [] });
    const assignSpy = vi.spyOn(window.location, 'assign').mockImplementation(() => {});
    const wrapper = mountSection(true);
    await flushPromises();

    clickBodyButton('パスワードを変更');
    await flushPromises();

    await wrapper.find('#password-current').setValue('OldPassword123');
    await wrapper.find('#password-next').setValue('NewPassword123');
    await wrapper.find('#password-confirm').setValue('NewPassword123');
    await wrapper.find('form').trigger('submit');
    await flushPromises();

    expect(assignSpy).toHaveBeenCalledWith('/signin?password_changed=1');
  });
});

describe('AuthMethodsSection の OAuth 連携', () => {
  it('連携済みプロバイダーの内訳を出す', async () => {
    stubFetch({
      connections: [
        connection({
          provider: 'gitlab_selfhosted',
          provider_email: 'dev@example.com',
          instance_url: INSTANCE_URL,
        }),
      ],
      providers: [{ provider: 'gitlab_selfhosted', requires_instance_url: true }],
    });
    mountSection(true);
    await flushPromises();

    expect(document.body.textContent).toContain('GitLab (セルフホスト)');
    expect(document.body.textContent).toContain('dev@example.com');
    expect(document.body.textContent).toContain('接続日時');
    expect(document.body.textContent).toContain(INSTANCE_URL);
    // 連携済みなので「追加できる連携」には出さない
    expect(document.body.textContent).not.toContain('追加できる連携');
  });

  it('未連携のプロバイダーだけ追加候補に出す', async () => {
    stubFetch({
      connections: [connection({ provider: 'github' })],
      providers: [
        { provider: 'github', requires_instance_url: false },
        { provider: 'google', requires_instance_url: false },
      ],
    });
    mountSection(true);
    await flushPromises();

    expect(document.body.textContent).toContain('追加できる連携');
    expect(document.body.textContent).toContain('Google');
  });

  it('self-hosted はインスタンス URL 未入力だと連携ボタンを押せない', async () => {
    stubFetch({
      connections: [],
      providers: [{ provider: 'gitlab_selfhosted', requires_instance_url: true }],
    });
    const assignSpy = vi.spyOn(window.location, 'assign').mockImplementation(() => {});
    const wrapper = mountSection(true);
    await flushPromises();

    const button = bodyButton('連携する') as HTMLButtonElement;
    expect(button.disabled).toBe(true);
    button.click();
    expect(assignSpy).not.toHaveBeenCalled();

    await wrapper.find('#instance-gitlab_selfhosted').setValue(INSTANCE_URL);
    await flushPromises();

    clickBodyButton('連携する');
    expect(assignSpy).toHaveBeenCalledWith(
      `/api/v1/auth/oauth/gitlab_selfhosted?redirect_after=%2Fsettings%2Fsecurity%3Flinked%3Dgitlab_selfhosted&error_redirect_after=%2Fsettings%2Fsecurity&instance_url=${encodeURIComponent(INSTANCE_URL)}`,
    );
  });

  it('確認してから解除し、instance_url を添えて送る', async () => {
    const { deleted } = stubFetch({
      connections: [
        connection({ provider: 'gitlab_selfhosted', instance_url: INSTANCE_URL }),
        connection({ provider: 'github' }),
      ],
      providers: [
        { provider: 'gitlab_selfhosted', requires_instance_url: true },
        { provider: 'github', requires_instance_url: false },
      ],
    });
    mountSection(true);
    await flushPromises();

    // 確認を挟むまでは何も送らない
    [...document.body.querySelectorAll('button')]
      .filter((b) => b.textContent?.trim() === '解除')[0]
      .click();
    await flushPromises();
    expect(deleted).toEqual([]);

    clickBodyButton('解除する');
    await flushPromises();

    expect(deleted).toEqual([
      `/api/v1/auth/oauth/connections/gitlab_selfhosted?instance_url=${encodeURIComponent(INSTANCE_URL)}`,
    ]);
    expect(document.body.textContent).toContain('GitLab (セルフホスト) の連携を解除しました。');
  });

  it('最後の認証方法はサーバーの拒否をそのまま伝える', async () => {
    stubFetch({
      connections: [connection({ provider: 'github' })],
      providers: [{ provider: 'github', requires_instance_url: false }],
      disconnectError: { status: 403, message: 'oauth-last-auth-method' },
    });
    mountSection(false);
    await flushPromises();

    clickBodyButton('解除');
    await flushPromises();
    clickBodyButton('解除する');
    await flushPromises();

    expect(document.body.textContent).toContain('これが最後の認証方法のため解除できません。');
  });

  it('認証方法が1つだけなら解除前に注意を出す', async () => {
    stubFetch({
      connections: [connection({ provider: 'github' })],
      providers: [{ provider: 'github', requires_instance_url: false }],
    });
    mountSection(false);
    await flushPromises();

    expect(document.body.textContent).toContain('これが最後の認証方法の可能性があります。');
  });

  /** パスキーも認証方法。数え落とすと最後でないのに注意が出る。 */
  it('パスキーがあれば最後の認証方法の注意を出さない', async () => {
    stubFetch({
      connections: [connection({ provider: 'github' })],
      providers: [{ provider: 'github', requires_instance_url: false }],
      passkeyCount: 1,
    });
    mountSection(false);
    await flushPromises();

    expect(document.body.textContent).not.toContain('これが最後の認証方法の可能性があります。');
  });
});

describe('AuthMethodsSection の ?linked= の扱い', () => {
  /** 連携から戻った直後の URL を作る。history.state も載せて引き継ぎを見る。 */
  function enterWith(search: string, state: unknown = { vike: 'routed' }) {
    window.history.replaceState(state, '', `/settings/security${search}`);
  }

  it('知っているプロバイダーなら成功通知を出し、印を URL から落とす', async () => {
    stubFetch({ connections: [connection()], providers: [] });
    enterWith('?linked=github');
    const wrapper = mountSection();
    await flushPromises();

    expect(wrapper.text()).toContain('GitHub を連携しました。');
    expect(window.location.search).toBe('');
  });

  // URL から拾った文字列をそのまま通知文へ入れると、その画面を開かせるだけで
  // 正規の設定画面が出した通知として任意の文面を読ませられる
  it('知らない値は通知に出さない', async () => {
    stubFetch({ connections: [], providers: [] });
    enterWith('?linked=%E5%81%BD%E3%81%AE%E3%81%8A%E7%9F%A5%E3%82%89%E3%81%9B');
    const wrapper = mountSection();
    await flushPromises();

    expect(wrapper.text()).not.toContain('偽のお知らせ');
    expect(wrapper.text()).not.toContain('を連携しました。');
  });

  it('知らない値でも印は URL から落とす（再読み込みで残さない）', async () => {
    stubFetch({ connections: [], providers: [] });
    enterWith('?linked=bogus');
    mountSection();
    await flushPromises();

    expect(window.location.search).toBe('');
  });

  it('印を落とすときに history.state を捨てない', async () => {
    stubFetch({ connections: [connection()], providers: [] });
    enterWith('?linked=github', { vike: 'routed' });
    mountSection();
    await flushPromises();

    expect(window.history.state).toEqual({ vike: 'routed' });
  });
});
