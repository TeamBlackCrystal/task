import { describe, it, expect, afterEach, vi } from 'vitest';
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils';
import { VueQueryPlugin, QueryClient } from '@tanstack/vue-query';
import OAuthButtons from '../OAuthButtons.vue';

type Provider = { provider: string; requires_instance_url: boolean };

const jsonResponse = (data: unknown, status = 200) =>
  new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });

function stubProviders(providers: Provider[]) {
  const fetchMock = vi.fn(async (req: Request | string) => {
    const url = typeof req === 'string' ? req : req.url;
    const pathname = new URL(url, 'http://localhost').pathname;
    if (pathname.endsWith('/v1/auth/oauth/providers')) {
      return jsonResponse({ providers });
    }
    return jsonResponse({ message: 'not-found' }, 404);
  });
  vi.stubGlobal('fetch', fetchMock);
  return fetchMock;
}

function mountButtons(props: Record<string, unknown> = {}) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return mount(OAuthButtons, {
    props,
    global: { plugins: [[VueQueryPlugin, { queryClient }]] },
    attachTo: document.body,
  });
}

function bodyButton(label: string) {
  return [...document.body.querySelectorAll('button')].find((b) => b.textContent?.trim() === label);
}

enableAutoUnmount(afterEach);

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('OAuthButtons', () => {
  it('有効なプロバイダーごとにボタンを描画する', async () => {
    stubProviders([
      { provider: 'github', requires_instance_url: false },
      { provider: 'gitlab', requires_instance_url: false },
      { provider: 'gitlab_selfhosted', requires_instance_url: true },
    ]);
    mountButtons();
    await flushPromises();

    expect(bodyButton('GitHub で続ける')).toBeTruthy();
    expect(bodyButton('GitLab で続ける')).toBeTruthy();
    expect(bodyButton('GitLab (セルフホスト) で続ける')).toBeTruthy();
  });

  it('プロバイダーが無ければ区切りもボタンも描画しない', async () => {
    stubProviders([]);
    mountButtons();
    await flushPromises();

    expect(document.body.textContent).not.toContain('または');
    expect(bodyButton('GitHub で続ける')).toBeUndefined();
  });

  it('gitlab.com ボタンで OAuth 開始 URL へフルページ遷移する', async () => {
    stubProviders([{ provider: 'gitlab', requires_instance_url: false }]);
    const assignSpy = vi.spyOn(window.location, 'assign').mockImplementation(() => {});
    mountButtons();
    await flushPromises();

    bodyButton('GitLab で続ける')!.click();

    expect(assignSpy).toHaveBeenCalledWith(
      '/api/v1/auth/oauth/gitlab?redirect_after=%2F&error_redirect_after=%2F',
    );
  });

  it('redirect-after prop を遷移 URL に反映する', async () => {
    stubProviders([{ provider: 'github', requires_instance_url: false }]);
    const assignSpy = vi.spyOn(window.location, 'assign').mockImplementation(() => {});
    mountButtons({ redirectAfter: '/dashboard' });
    await flushPromises();

    bodyButton('GitHub で続ける')!.click();

    expect(assignSpy).toHaveBeenCalledWith(
      '/api/v1/auth/oauth/github?redirect_after=%2Fdashboard&error_redirect_after=%2F',
    );
  });

  it('error-redirect-after prop を遷移 URL に反映する', async () => {
    stubProviders([{ provider: 'gitlab', requires_instance_url: false }]);
    const assignSpy = vi.spyOn(window.location, 'assign').mockImplementation(() => {});
    mountButtons({ errorRedirectAfter: '/signin' });
    await flushPromises();

    bodyButton('GitLab で続ける')!.click();

    expect(assignSpy).toHaveBeenCalledWith(
      '/api/v1/auth/oauth/gitlab?redirect_after=%2F&error_redirect_after=%2Fsignin',
    );
  });

  it('self-hosted は URL 未入力ではボタンを無効化し遷移しない', async () => {
    stubProviders([{ provider: 'gitlab_selfhosted', requires_instance_url: true }]);
    const assignSpy = vi.spyOn(window.location, 'assign').mockImplementation(() => {});
    mountButtons();
    await flushPromises();

    const button = bodyButton('GitLab (セルフホスト) で続ける');
    expect(button?.disabled).toBe(true);
    button!.click();
    expect(assignSpy).not.toHaveBeenCalled();
  });

  it('self-hosted は instance_url を encode して付与する', async () => {
    stubProviders([{ provider: 'gitlab_selfhosted', requires_instance_url: true }]);
    const assignSpy = vi.spyOn(window.location, 'assign').mockImplementation(() => {});
    const wrapper = mountButtons();
    await flushPromises();

    await wrapper.find('input[type="url"]').setValue('https://gitlab.example.com');

    const button = bodyButton('GitLab (セルフホスト) で続ける');
    expect(button?.disabled).toBe(false);
    button!.click();

    expect(assignSpy).toHaveBeenCalledWith(
      '/api/v1/auth/oauth/gitlab_selfhosted?redirect_after=%2F&error_redirect_after=%2F&instance_url=https%3A%2F%2Fgitlab.example.com',
    );
  });
});
