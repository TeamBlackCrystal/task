import { describe, it, expect, afterEach, vi } from 'vitest';
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils';
import { VueQueryPlugin, QueryClient } from '@tanstack/vue-query';
import AuthMethodsSection from '../AuthMethodsSection.vue';
import { OAUTH_LINK_NOTICE } from '@/lib/one-time-notice';
import type { components } from '@/generated/api';

const USER_ID = '00000000-0000-0000-0000-000000000001';
const INSTANCE_URL = 'https://gitlab.example.com';
/** 汎用 OIDC の連携は issuer 込みで保存される（backend の `db_provider_key`）。 */
const OIDC_CONNECTION = 'oidc:https://idp.example.com';

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

/** `/oauth/providers` の 1 件。OIDC だけ連携一覧の識別子が slug と違う。 */
function provider(slug: string, connectionProvider = slug): Provider {
  return {
    provider: slug,
    connection_provider: connectionProvider,
    requires_instance_url: slug === 'gitlab_selfhosted',
  };
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
  // 通知の印はタブに残るので、テスト間で持ち越さない
  window.sessionStorage.clear();
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

    expect(assignSpy).toHaveBeenCalledWith('/signin');
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
      providers: [provider('gitlab_selfhosted')],
    });
    mountSection(true);
    await flushPromises();

    expect(document.body.textContent).toContain('GitLab (セルフホスト)');
    expect(document.body.textContent).toContain('dev@example.com');
    expect(document.body.textContent).toContain('接続日時');
    expect(document.body.textContent).toContain(INSTANCE_URL);
    // インスタンスが違えば別の連携なので、1 件連携済みでも追加の口は残す
    expect(document.body.textContent).toContain('追加できる連携');
  });

  it('self-hosted は連携済みでも別のインスタンスを開始できる', async () => {
    stubFetch({
      connections: [connection({ provider: 'gitlab_selfhosted', instance_url: INSTANCE_URL })],
      providers: [provider('gitlab_selfhosted')],
    });
    const assignSpy = vi.spyOn(window.location, 'assign').mockImplementation(() => {});
    const wrapper = mountSection(true);
    await flushPromises();

    const other = 'https://gitlab.internal.example.com';
    await wrapper.find('#instance-gitlab_selfhosted').setValue(other);
    await flushPromises();

    expect((bodyButton('連携する') as HTMLButtonElement).disabled).toBe(false);
    clickBodyButton('連携する');

    expect(assignSpy).toHaveBeenCalledWith(
      `/api/v1/auth/oauth/gitlab_selfhosted?redirect_after=%2Fsettings%2Fsecurity&error_redirect_after=%2Fsettings%2Fsecurity&instance_url=${encodeURIComponent(other)}`,
    );
  });

  it('連携済みと同じインスタンスは開始しない', async () => {
    stubFetch({
      connections: [connection({ provider: 'gitlab_selfhosted', instance_url: INSTANCE_URL })],
      providers: [provider('gitlab_selfhosted')],
    });
    const assignSpy = vi.spyOn(window.location, 'assign').mockImplementation(() => {});
    const wrapper = mountSection(true);
    await flushPromises();

    // 末尾のスラッシュ違いは同じインスタンスとして扱う
    await wrapper.find('#instance-gitlab_selfhosted').setValue(`${INSTANCE_URL}/`);
    await flushPromises();

    const button = bodyButton('連携する') as HTMLButtonElement;
    expect(button.disabled).toBe(true);
    button.click();
    expect(assignSpy).not.toHaveBeenCalled();
    expect(document.body.textContent).toContain('このインスタンスは連携済みです');
  });

  // 重複の判定は backend も保存した文字列の完全一致。画面が origin まで正規化して
  // ホスト名の大小を同一視すると、backend では足せるインスタンスを止めてしまう
  it('ホスト名の大小が違うインスタンスは別の連携として開始できる', async () => {
    stubFetch({
      connections: [
        connection({ provider: 'gitlab_selfhosted', instance_url: 'https://GitLab.example.com' }),
      ],
      providers: [provider('gitlab_selfhosted')],
    });
    const assignSpy = vi.spyOn(window.location, 'assign').mockImplementation(() => {});
    const wrapper = mountSection(true);
    await flushPromises();

    await wrapper.find('#instance-gitlab_selfhosted').setValue(INSTANCE_URL);
    await flushPromises();

    expect(document.body.textContent).not.toContain('このインスタンスは連携済みです');
    expect((bodyButton('連携する') as HTMLButtonElement).disabled).toBe(false);
    clickBodyButton('連携する');
    expect(assignSpy).toHaveBeenCalledWith(
      `/api/v1/auth/oauth/gitlab_selfhosted?redirect_after=%2Fsettings%2Fsecurity&error_redirect_after=%2Fsettings%2Fsecurity&instance_url=${encodeURIComponent(INSTANCE_URL)}`,
    );
  });

  it('未連携のプロバイダーだけ追加候補に出す', async () => {
    stubFetch({
      connections: [connection({ provider: 'github' })],
      providers: [provider('github'), provider('google')],
    });
    mountSection(true);
    await flushPromises();

    expect(document.body.textContent).toContain('追加できる連携');
    expect(document.body.textContent).toContain('Google');
  });

  it('self-hosted はインスタンス URL 未入力だと連携ボタンを押せない', async () => {
    stubFetch({
      connections: [],
      providers: [provider('gitlab_selfhosted')],
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
      `/api/v1/auth/oauth/gitlab_selfhosted?redirect_after=%2Fsettings%2Fsecurity&error_redirect_after=%2Fsettings%2Fsecurity&instance_url=${encodeURIComponent(INSTANCE_URL)}`,
    );
  });

  it('確認してから解除し、instance_url を添えて送る', async () => {
    const { deleted } = stubFetch({
      connections: [
        connection({ provider: 'gitlab_selfhosted', instance_url: INSTANCE_URL }),
        connection({ provider: 'github' }),
      ],
      providers: [provider('gitlab_selfhosted'), provider('github')],
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
      providers: [provider('github')],
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
      providers: [provider('github')],
    });
    mountSection(false);
    await flushPromises();

    expect(document.body.textContent).toContain('これが最後の認証方法の可能性があります。');
  });

  // backend は OIDC の連携を `oidc:{issuer}` で保存する。開始用 slug の `oidc` で
  // 突き合わせると、連携済みでも候補に残り続ける
  it('OIDC は連携済みなら追加候補に出さない', async () => {
    stubFetch({
      connections: [connection({ provider: OIDC_CONNECTION })],
      providers: [provider('oidc', OIDC_CONNECTION)],
    });
    mountSection(true);
    await flushPromises();

    // issuer 付きの生の識別子ではなく表示名で出る
    expect(document.body.textContent).toContain('OIDC');
    expect(document.body.textContent).not.toContain('追加できる連携');
  });

  /** パスキーも認証方法。数え落とすと最後でないのに注意が出る。 */
  it('パスキーがあれば最後の認証方法の注意を出さない', async () => {
    stubFetch({
      connections: [connection({ provider: 'github' })],
      providers: [provider('github')],
      passkeyCount: 1,
    });
    mountSection(false);
    await flushPromises();

    expect(document.body.textContent).not.toContain('これが最後の認証方法の可能性があります。');
  });
});

describe('AuthMethodsSection の成功通知', () => {
  /** 連携から戻った直後の URL を作る。history.state も載せて引き継ぎを見る。 */
  function enterWith(search: string, state: unknown = { vike: 'routed' }) {
    window.history.replaceState(state, '', `/settings/security${search}`);
  }

  /** 「連携する」を押した側が置く印。連携一覧で突き合わせる識別子まで含む。 */
  function markStarted(slug: string, connectionProvider = slug, instanceUrl = '') {
    window.sessionStorage.setItem(
      OAUTH_LINK_NOTICE,
      JSON.stringify({ provider: slug, connectionProvider, instanceUrl }),
    );
  }

  it('自分で始めた連携が一覧に入っていれば通知を出す', async () => {
    stubFetch({ connections: [connection()], providers: [] });
    markStarted('github');
    enterWith('');
    const wrapper = mountSection();
    await flushPromises();

    expect(wrapper.text()).toContain('GitHub を連携しました。');
  });

  // URL を開かせるだけで正規の成功通知を出せると、連携していない人に
  // 連携できたと思わせられる。通知の根拠は本人のタブに置いた印だけにする
  it('印が無ければ URL に linked を付けても通知を出さない', async () => {
    stubFetch({ connections: [connection()], providers: [] });
    enterWith('?linked=github');
    const wrapper = mountSection();
    await flushPromises();

    expect(wrapper.text()).not.toContain('を連携しました。');
  });

  // 承認の途中で失敗するとこの画面には戻らない。印だけを根拠にすると、
  // 次にここを開いた時点で連携できたことになってしまう
  it('印があっても一覧に入っていなければ通知を出さない', async () => {
    stubFetch({ connections: [], providers: [] });
    markStarted('github');
    enterWith('');
    const wrapper = mountSection();
    await flushPromises();

    expect(wrapper.text()).not.toContain('を連携しました。');
  });

  // 開始用 slug は `oidc` だが、連携一覧は `oidc:{issuer}` を返す。slug で突き合わせると
  // 連携できていても通知が出ない
  it('OIDC は issuer 付きの識別子で一覧と突き合わせる', async () => {
    stubFetch({ connections: [connection({ provider: OIDC_CONNECTION })], providers: [] });
    markStarted('oidc', OIDC_CONNECTION);
    enterWith('');
    const wrapper = mountSection();
    await flushPromises();

    expect(wrapper.text()).toContain('OIDC を連携しました。');
  });

  // インスタンス A を連携済みのまま B の承認を中断して開き直すと、B は連携していない。
  // プロバイダー名だけで突き合わせると A を見て成功と判定してしまう
  it('別のインスタンスが連携済みでも、開始した方が入るまで通知しない', async () => {
    stubFetch({
      connections: [connection({ provider: 'gitlab_selfhosted', instance_url: INSTANCE_URL })],
      providers: [],
    });
    markStarted('gitlab_selfhosted', 'gitlab_selfhosted', 'https://gitlab.internal.example.com');
    enterWith('');
    const wrapper = mountSection();
    await flushPromises();

    expect(wrapper.text()).not.toContain('を連携しました。');
  });

  it('開始したインスタンスが一覧に入っていれば通知を出す', async () => {
    stubFetch({
      connections: [connection({ provider: 'gitlab_selfhosted', instance_url: INSTANCE_URL })],
      providers: [],
    });
    // 末尾のスラッシュ違いは同じインスタンスとして扱う
    markStarted('gitlab_selfhosted', 'gitlab_selfhosted', `${INSTANCE_URL}/`);
    enterWith('');
    const wrapper = mountSection();
    await flushPromises();

    expect(wrapper.text()).toContain('GitLab (セルフホスト) を連携しました。');
  });

  it('印は一度きりで、開き直しても通知は出ない', async () => {
    stubFetch({ connections: [connection()], providers: [] });
    markStarted('github');
    enterWith('');
    mountSection();
    await flushPromises();

    const second = mountSection();
    await flushPromises();

    expect(second.text()).not.toContain('を連携しました。');
  });

  it('プロバイダー側の失敗は通知に変えない', async () => {
    stubFetch({ connections: [connection()], providers: [] });
    markStarted('github');
    enterWith('?oauth_error=access_denied');
    const wrapper = mountSection();
    await flushPromises();

    expect(wrapper.text()).not.toContain('を連携しました。');
    expect(wrapper.text()).toContain('外部プロバイダーでの連携に失敗しました。');
    expect(window.location.search).toBe('');
  });

  it('失敗の印を落とすときに history.state を捨てない', async () => {
    stubFetch({ connections: [connection()], providers: [] });
    enterWith('?oauth_error=access_denied', { vike: 'routed' });
    mountSection();
    await flushPromises();

    expect(window.history.state).toEqual({ vike: 'routed' });
  });
});
