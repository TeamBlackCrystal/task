import { describe, it, expect, afterEach, vi } from 'vitest';
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils';
import { VueQueryPlugin, QueryClient } from '@tanstack/vue-query';
import ProfileForm from '../ProfileForm.vue';
import type { components } from '@/generated/api';

const user: components['schemas']['UserResponse'] = {
  id: '00000000-0000-0000-0000-000000000001',
  username: 'tester',
  email: 'tester@example.com',
  email_verified: true,
  is_admin: false,
  is_suspended: false,
  totp_enabled: false,
  bio: '最初の説明',
  avatar_url: 'https://example.com/a.png',
};

/** PATCH /v1/auth/me に届いたリクエストボディを記録する。 */
function stubPatchMe(status = 200) {
  const bodies: unknown[] = [];
  const fetchMock = vi.fn(async (req: Request) => {
    const pathname = new URL(req.url, 'http://localhost').pathname;
    if (pathname.endsWith('/v1/auth/me') && req.method === 'PATCH') {
      bodies.push(await req.clone().json());
      return new Response(JSON.stringify(user), {
        status,
        headers: { 'Content-Type': 'application/json' },
      });
    }
    return new Response(JSON.stringify({ message: 'not-found' }), { status: 404 });
  });
  vi.stubGlobal('fetch', fetchMock);
  return bodies;
}

function mountForm() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return mount(ProfileForm, {
    props: { user },
    global: { plugins: [[VueQueryPlugin, { queryClient }]] },
    attachTo: document.body,
  });
}

enableAutoUnmount(afterEach);

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('ProfileForm', () => {
  it('アバター欄を空にすると clear_avatar_url を送る', async () => {
    const bodies = stubPatchMe();
    const wrapper = mountForm();
    await flushPromises();

    await wrapper.find('#avatarUrl').setValue('');
    await wrapper.find('form').trigger('submit');
    await flushPromises();

    expect(bodies).toHaveLength(1);
    expect(bodies[0]).toMatchObject({ clear_avatar_url: true });
    expect(bodies[0]).not.toHaveProperty('avatar_url');
  });

  it('URL を入力すると avatar_url を送る', async () => {
    const bodies = stubPatchMe();
    const wrapper = mountForm();
    await flushPromises();

    await wrapper.find('#avatarUrl').setValue('https://example.com/b.png');
    await wrapper.find('#username').setValue('renamed');
    await wrapper.find('#bio').setValue('新しい説明');
    await wrapper.find('form').trigger('submit');
    await flushPromises();

    expect(bodies[0]).toEqual({
      username: 'renamed',
      bio: '新しい説明',
      avatar_url: 'https://example.com/b.png',
    });
  });

  it('http/https でない URL は送信せずエラーを出す', async () => {
    const bodies = stubPatchMe();
    const wrapper = mountForm();
    await flushPromises();

    await wrapper.find('#avatarUrl').setValue('javascript:alert(1)');
    await wrapper.find('form').trigger('submit');
    await flushPromises();

    expect(bodies).toHaveLength(0);
    expect(wrapper.text()).toContain('http:// または https:// で始まる URL');
  });

  it('3 文字未満のユーザー名は送信せずエラーを出す', async () => {
    const bodies = stubPatchMe();
    const wrapper = mountForm();
    await flushPromises();

    await wrapper.find('#username').setValue('ab');
    await wrapper.find('form').trigger('submit');
    await flushPromises();

    expect(bodies).toHaveLength(0);
    expect(wrapper.text()).toContain('3文字以上で入力してください。');
  });

  it('前後の空白は取り除いて送る', async () => {
    const bodies = stubPatchMe();
    const wrapper = mountForm();
    await flushPromises();

    await wrapper.find('#avatarUrl').setValue('  https://example.com/b.png  ');
    await wrapper.find('form').trigger('submit');
    await flushPromises();

    expect(bodies).toHaveLength(1);
    expect(bodies[0]).toMatchObject({ avatar_url: 'https://example.com/b.png' });
  });

  it('保存に失敗したらエラーを表示する', async () => {
    stubPatchMe(500);
    const wrapper = mountForm();
    await flushPromises();

    await wrapper.find('form').trigger('submit');
    await flushPromises();

    expect(wrapper.text()).toContain('保存できませんでした。');
  });

  it('400 のときは入力内容の確認を促す', async () => {
    stubPatchMe(400);
    const wrapper = mountForm();
    await flushPromises();

    await wrapper.find('form').trigger('submit');
    await flushPromises();

    expect(wrapper.text()).toContain('入力内容を確認してください。');
  });

  it('保存後に編集を再開すると「保存しました。」が消える', async () => {
    stubPatchMe();
    const wrapper = mountForm();
    await flushPromises();

    await wrapper.find('form').trigger('submit');
    await flushPromises();
    expect(wrapper.text()).toContain('保存しました。');

    await wrapper.find('#bio').setValue('編集を再開する');
    await flushPromises();

    expect(wrapper.text()).not.toContain('保存しました。');
  });
});
