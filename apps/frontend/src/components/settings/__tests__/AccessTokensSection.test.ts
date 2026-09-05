import { describe, it, expect, afterEach, vi } from 'vitest';
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils';
import { VueQueryPlugin, QueryClient } from '@tanstack/vue-query';
import AccessTokensSection from '../AccessTokensSection.vue';
import type { components } from '@/generated/api';

const USER_ID = '00000000-0000-0000-0000-000000000001';
const OTHER_USER_ID = '00000000-0000-0000-0000-000000000002';
const TENANT_ID = '11111111-1111-1111-1111-111111111111';

const user: components['schemas']['UserResponse'] = {
  id: USER_ID,
  username: 'tester',
  email: 'tester@example.com',
  email_verified: true,
  is_admin: false,
  is_suspended: false,
  totp_enabled: false,
  has_password: true,
  bio: null,
  avatar_url: null,
};

function tenant(ownerId: string): components['schemas']['TenantResponse'] {
  return {
    id: TENANT_ID,
    display_id: 'acme',
    name: 'Acme Inc',
    description: '',
    icon_url: '',
    owner_id: ownerId,
    drive_quota_bytes: null,
    require_2fa: false,
  };
}

function token(
  overrides: Partial<components['schemas']['PersonalTokenResponse']> = {},
): components['schemas']['PersonalTokenResponse'] {
  return {
    id: '22222222-2222-2222-2222-222222222222',
    name: 'CLI on MacBook',
    token_last_four: '7f3a',
    tenant_id: TENANT_ID,
    project_ids: null,
    scopes: ['read:task', 'write:task', 'read:project'],
    expires_at: null,
    last_used_at: null,
    revoked: false,
    user_id: USER_ID,
    ...overrides,
  };
}

type MockState = {
  tokens: components['schemas']['PersonalTokenResponse'][];
  tenantOwnerId?: string;
  createStatus?: number;
  deleteStatus?: number;
};

const jsonResponse = (data: unknown, status = 200) =>
  new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });

function stubFetch(state: MockState) {
  const createBodies: unknown[] = [];
  const deletedPaths: string[] = [];
  const fetchMock = vi.fn(async (req: Request) => {
    const pathname = new URL(req.url, 'http://localhost').pathname;

    if (req.method === 'GET' && pathname.endsWith('/v1/tenants')) {
      return jsonResponse([tenant(state.tenantOwnerId ?? USER_ID)]);
    }
    if (req.method === 'GET' && pathname.endsWith('/v1/personal_tokens')) {
      return jsonResponse(state.tokens);
    }
    if (req.method === 'POST' && pathname.endsWith('/v1/personal_tokens')) {
      const body = (await req.clone().json()) as Record<string, unknown>;
      createBodies.push(body);
      if (state.createStatus) return jsonResponse({ message: 'error' }, state.createStatus);
      const created = token({ name: String(body.name), token_last_four: 'b21c' });
      state.tokens = [...state.tokens, created];
      return jsonResponse({ ...created, token: 'pat_plain-token-value' }, 201);
    }
    if (req.method === 'DELETE' && pathname.includes('/v1/personal_tokens/')) {
      deletedPaths.push(pathname);
      if (state.deleteStatus) return jsonResponse({ message: 'error' }, state.deleteStatus);
      const id = pathname.split('/').pop();
      state.tokens = state.tokens.filter((t) => t.id !== id);
      return new Response(null, { status: 204 });
    }
    return jsonResponse({ message: 'not-found' }, 404);
  });
  vi.stubGlobal('fetch', fetchMock);
  return { createBodies, deletedPaths };
}

function mountSection() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return mount(AccessTokensSection, {
    props: { user },
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

/** スコープのチェックボックスをラベルの scope 文字列で探してクリックする。 */
function clickScopeCheckbox(scope: string) {
  const label = [...document.body.querySelectorAll('label')].find((l) =>
    l.textContent?.includes(scope),
  );
  const checkbox = label?.querySelector<HTMLButtonElement>('[data-slot="checkbox"]');
  if (!checkbox) throw new Error(`scope checkbox "${scope}" not found`);
  checkbox.click();
}

enableAutoUnmount(afterEach);

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('AccessTokensSection', () => {
  it('トークン一覧に伏せ字・スコープ数・有効期限・最終使用を表示する', async () => {
    stubFetch({
      tokens: [
        token({ last_used_at: '2026-01-01T00:00:00Z' }),
        token({
          id: '33333333-3333-3333-3333-333333333333',
          name: 'CI deploy',
          token_last_four: 'b21c',
          scopes: ['read:task'],
          expires_at: '2099-11-14T00:00:00Z',
        }),
      ],
    });
    const wrapper = mountSection();
    await flushPromises();

    const text = wrapper.text();
    expect(text).toContain('CLI on MacBook');
    expect(text).toContain('pat_••••••7f3a');
    expect(text).toContain('Acme Inc');
    expect(text).toContain('3 スコープ');
    expect(text).toContain('無期限');
    expect(text).toContain('に使用');
    expect(text).toContain('CI deploy');
    expect(text).toContain('1 スコープ');
    expect(text).toContain('まで有効');
    expect(text).toContain('未使用');
  });

  it('トークンが無ければ空メッセージを表示する', async () => {
    stubFetch({ tokens: [] });
    const wrapper = mountSection();
    await flushPromises();

    expect(wrapper.text()).toContain('トークンはまだありません。');
  });

  it('発行フォームから name / tenant_id / scopes / expires_at(90日) を送る', async () => {
    const { createBodies } = stubFetch({ tokens: [] });
    const wrapper = mountSection();
    await flushPromises();

    clickBodyButton('トークンを発行');
    await flushPromises();

    // トークン 0 件でフォームを開いている間、空の一覧ボックスを描画しない
    expect(wrapper.find('[data-testid="token-list"]').exists()).toBe(false);

    await wrapper.find('#token-name').setValue('CI deploy');
    clickScopeCheckbox('read:task');
    clickScopeCheckbox('write:task');
    await flushPromises();

    await wrapper.find('form').trigger('submit');
    await flushPromises();

    expect(createBodies).toHaveLength(1);
    const body = createBodies[0] as {
      name: string;
      tenant_id: string;
      scopes: string[];
      expires_at: string;
    };
    expect(body.name).toBe('CI deploy');
    expect(body.tenant_id).toBe(TENANT_ID);
    expect(body.scopes).toEqual(['read:task', 'write:task']);
    // 既定の有効期限は 90 日
    const days = (new Date(body.expires_at).getTime() - new Date().getTime()) / 86400000;
    expect(days).toBeGreaterThan(89.9);
    expect(days).toBeLessThan(90.1);

    // 平文トークンは発行直後の画面にだけ表示される
    expect(wrapper.find('[data-testid="created-token"]').text()).toBe('pat_plain-token-value');
    // 一覧も更新される
    expect(wrapper.text()).toContain('pat_••••••b21c');
  });

  it('無期限を選ぶと expires_at は null になる', async () => {
    const { createBodies } = stubFetch({ tokens: [] });
    const wrapper = mountSection();
    await flushPromises();

    clickBodyButton('トークンを発行');
    await flushPromises();

    await wrapper.find('#token-name').setValue('forever');
    clickScopeCheckbox('read:task');
    clickBodyButton('無期限');
    await flushPromises();

    await wrapper.find('form').trigger('submit');
    await flushPromises();

    expect(createBodies).toHaveLength(1);
    expect((createBodies[0] as { expires_at: unknown }).expires_at).toBeNull();
  });

  it('トークン名が空なら送信しない', async () => {
    const { createBodies } = stubFetch({ tokens: [] });
    const wrapper = mountSection();
    await flushPromises();

    clickBodyButton('トークンを発行');
    await flushPromises();

    clickScopeCheckbox('read:task');
    await flushPromises();
    await wrapper.find('form').trigger('submit');
    await flushPromises();

    expect(createBodies).toHaveLength(0);
    expect(wrapper.text()).toContain('トークン名を入力してください。');
  });

  it('101 文字のトークン名は送信しない（境界の 1 つ外）', async () => {
    const { createBodies } = stubFetch({ tokens: [] });
    const wrapper = mountSection();
    await flushPromises();

    clickBodyButton('トークンを発行');
    await flushPromises();

    await wrapper.find('#token-name').setValue('a'.repeat(101));
    clickScopeCheckbox('read:task');
    await flushPromises();
    await wrapper.find('form').trigger('submit');
    await flushPromises();

    expect(createBodies).toHaveLength(0);
    expect(wrapper.text()).toContain('100文字以内で入力してください。');
  });

  it('文字数は UTF-16 ではなくコードポイント単位で数える（絵文字 100 個は通り、101 個は弾く）', async () => {
    const { createBodies } = stubFetch({ tokens: [] });
    const wrapper = mountSection();
    await flushPromises();

    clickBodyButton('トークンを発行');
    await flushPromises();

    // サロゲートペア 100 個 = UTF-16 では 200。UTF-16 で数えると backend が許す名前を画面が弾く
    await wrapper.find('#token-name').setValue('😀'.repeat(100));
    clickScopeCheckbox('read:task');
    await flushPromises();
    await wrapper.find('form').trigger('submit');
    await flushPromises();

    expect(createBodies).toHaveLength(1);

    clickBodyButton('トークンを発行');
    await flushPromises();
    await wrapper.find('#token-name').setValue('😀'.repeat(101));
    clickScopeCheckbox('read:task');
    await flushPromises();
    await wrapper.find('form').trigger('submit');
    await flushPromises();

    expect(createBodies).toHaveLength(1);
    expect(wrapper.text()).toContain('100文字以内で入力してください。');
  });

  it.each([
    ['成功', true, 'コピーしました'],
    ['失敗', false, 'コピーできませんでした。表示中のトークンを選択してコピーしてください。'],
  ])('平文トークンのコピー%s時に結果を表示する', async (_label, ok, expected) => {
    stubFetch({ tokens: [] });
    const writeText = ok
      ? vi.fn().mockResolvedValue(undefined)
      : vi.fn().mockRejectedValue(new Error('denied'));
    Object.defineProperty(globalThis.navigator, 'clipboard', {
      value: { writeText },
      configurable: true,
    });
    const wrapper = mountSection();
    await flushPromises();

    clickBodyButton('トークンを発行');
    await flushPromises();
    await wrapper.find('#token-name').setValue('CI deploy');
    clickScopeCheckbox('read:task');
    await flushPromises();
    await wrapper.find('form').trigger('submit');
    await flushPromises();

    clickBodyButton('コピー');
    await flushPromises();

    expect(writeText).toHaveBeenCalledWith('pat_plain-token-value');
    expect(wrapper.text()).toContain(expected);
  });

  it('スコープ未選択なら送信しない', async () => {
    const { createBodies } = stubFetch({ tokens: [] });
    const wrapper = mountSection();
    await flushPromises();

    clickBodyButton('トークンを発行');
    await flushPromises();

    await wrapper.find('#token-name').setValue('CI deploy');
    await wrapper.find('form').trigger('submit');
    await flushPromises();

    expect(createBodies).toHaveLength(0);
    expect(wrapper.text()).toContain('スコープを 1 つ以上選択してください。');
  });

  it('403 のときはオーナー限定であることを伝える', async () => {
    stubFetch({ tokens: [], createStatus: 403 });
    const wrapper = mountSection();
    await flushPromises();

    clickBodyButton('トークンを発行');
    await flushPromises();

    await wrapper.find('#token-name').setValue('CI deploy');
    clickScopeCheckbox('read:task');
    await flushPromises();
    await wrapper.find('form').trigger('submit');
    await flushPromises();

    expect(wrapper.text()).toContain('オーナーだけです。');
  });

  it('取り消しは確認ダイアログを経て DELETE を送り、一覧から消す', async () => {
    const { deletedPaths } = stubFetch({ tokens: [token()] });
    const wrapper = mountSection();
    await flushPromises();

    clickBodyButton('取り消し');
    await flushPromises();

    expect(document.body.textContent).toContain('トークンを取り消しますか？');
    // どのテナントの何を消すのか、ダイアログで判別できること
    expect(document.body.textContent).toContain('テナント「Acme Inc」の「CLI on MacBook」');

    clickBodyButton('取り消す');
    await flushPromises();

    expect(deletedPaths).toEqual(['/api/v1/personal_tokens/22222222-2222-2222-2222-222222222222']);
    expect(wrapper.text()).not.toContain('CLI on MacBook');
    expect(wrapper.text()).toContain('トークンはまだありません。');
  });

  it('確認ダイアログでキャンセルすると DELETE を送らない', async () => {
    const { deletedPaths } = stubFetch({ tokens: [token()] });
    const wrapper = mountSection();
    await flushPromises();

    clickBodyButton('取り消し');
    await flushPromises();
    clickBodyButton('キャンセル');
    await flushPromises();

    expect(deletedPaths).toHaveLength(0);
    expect(wrapper.text()).toContain('CLI on MacBook');
  });

  it('取り消しに失敗したらエラーを表示し、一覧は残る', async () => {
    stubFetch({ tokens: [token()], deleteStatus: 500 });
    const wrapper = mountSection();
    await flushPromises();

    clickBodyButton('取り消し');
    await flushPromises();
    clickBodyButton('取り消す');
    await flushPromises();

    expect(document.body.textContent).toContain('トークンを取り消せませんでした。');
    expect(wrapper.text()).toContain('CLI on MacBook');
  });

  it('自分がオーナーのテナントが無ければ発行ボタンを無効にする', async () => {
    stubFetch({ tokens: [], tenantOwnerId: OTHER_USER_ID });
    const wrapper = mountSection();
    await flushPromises();

    expect(wrapper.text()).toContain('自分がオーナーのテナントだけです。');
    expect(bodyButton('トークンを発行')?.disabled).toBe(true);
  });
});
