import { describe, it, expect, afterEach, vi } from 'vitest';
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils';
import { VueQueryPlugin, QueryClient } from '@tanstack/vue-query';
import ReviewRoundComposer from '../ReviewRoundComposer.vue';

const TENANT_ID = '11111111-1111-1111-1111-111111111111';
const PROJECT_ID = '00000000-0000-4000-8000-000000000010';

function stubFetch(status = 201) {
  const posted: unknown[] = [];
  const fetchMock = vi.fn(async (req: Request) => {
    if (req.method === 'POST' && req.url.includes('/reviews')) {
      posted.push(await req.clone().json());
      return new Response(JSON.stringify({ id: 'r-2', round: 2 }), {
        status,
        headers: { 'Content-Type': 'application/json' },
      });
    }
    return new Response(JSON.stringify({ message: 'not-found' }), { status: 404 });
  });
  vi.stubGlobal('fetch', fetchMock);
  return posted;
}

function mountComposer() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return mount(ReviewRoundComposer, {
    props: {
      tenantId: TENANT_ID,
      projectId: PROJECT_ID,
      prNumber: 618,
      nextRound: 2,
    },
    global: { plugins: [[VueQueryPlugin, { queryClient }]] },
    attachTo: document.body,
  });
}

function bodyButton(label: string) {
  return [...document.body.querySelectorAll('button')].find((b) =>
    b.textContent?.trim().startsWith(label),
  );
}

enableAutoUnmount(afterEach);

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('ReviewRoundComposer', () => {
  it('head SHA が未入力なら確定できない', async () => {
    stubFetch();
    mountComposer();
    await flushPromises();

    expect(bodyButton('確定')?.disabled).toBe(true);
  });

  it('下書きに積んでから確定すると 1 リクエストで一括作成する', async () => {
    const posted = stubFetch();
    const wrapper = mountComposer();
    await flushPromises();

    await wrapper.find('#composer-head').setValue('60cdd7795f94');
    await wrapper.find('#composer-summary').setValue('総評');

    // 1 件目
    await wrapper.find('#draft-title').setValue('認可漏れ');
    await wrapper.find('#draft-body').setValue('本文 1');
    await wrapper.find('#draft-file').setValue('src/App.vue');
    await wrapper.find('#draft-line').setValue('42');
    bodyButton('下書きに追加')!.click();
    await flushPromises();

    // 2 件目（位置情報なし）
    await wrapper.find('#draft-title').setValue('命名');
    await wrapper.find('#draft-body').setValue('本文 2');
    bodyButton('下書きに追加')!.click();
    await flushPromises();

    // 積んだだけではサーバーに何も作られない
    expect(posted).toHaveLength(0);
    expect(wrapper.get('[data-testid="staged-list"]').text()).toContain('認可漏れ');
    expect(wrapper.text()).toContain('下書きの指摘 2 件');

    bodyButton('確定')!.click();
    await flushPromises();

    expect(posted).toHaveLength(1);
    expect(posted[0]).toEqual({
      pr_number: 618,
      head_sha: '60cdd7795f94',
      summary: '総評',
      findings: [
        {
          severity: 'medium',
          title: '認可漏れ',
          body: '本文 1',
          file: 'src/App.vue',
          line: 42,
        },
        { severity: 'medium', title: '命名', body: '本文 2', file: null, line: null },
      ],
    });
    expect(wrapper.emitted('created')).toHaveLength(1);
  });

  it('指摘 0 件でも確定できる（総評だけのラウンド）', async () => {
    const posted = stubFetch();
    const wrapper = mountComposer();
    await flushPromises();

    await wrapper.find('#composer-head').setValue('abc');
    await wrapper.find('#composer-summary').setValue('具体的な不具合は見つからなかった');
    expect(wrapper.text()).toContain('指摘 0 件でも確定できます');

    bodyButton('確定')!.click();
    await flushPromises();

    expect(posted).toHaveLength(1);
    expect((posted[0] as { findings: unknown[] }).findings).toEqual([]);
  });

  it('タイトルと本文が空の指摘は下書きに積まない', async () => {
    stubFetch();
    const wrapper = mountComposer();
    await flushPromises();

    await wrapper.find('#draft-title').setValue('タイトルだけ');
    bodyButton('下書きに追加')!.click();
    await flushPromises();

    expect(wrapper.text()).toContain('タイトルと本文を入力してください。');
    expect(wrapper.find('[data-testid="staged-list"]').exists()).toBe(false);
  });

  it('行番号が数値でなければ積まない', async () => {
    stubFetch();
    const wrapper = mountComposer();
    await flushPromises();

    await wrapper.find('#draft-title').setValue('t');
    await wrapper.find('#draft-body').setValue('b');
    await wrapper.find('#draft-line').setValue('42行目');
    bodyButton('下書きに追加')!.click();
    await flushPromises();

    expect(wrapper.text()).toContain('行番号は数値で入力してください。');
    expect(wrapper.find('[data-testid="staged-list"]').exists()).toBe(false);
  });

  it('下書きから 1 件外せる', async () => {
    stubFetch();
    const wrapper = mountComposer();
    await flushPromises();

    await wrapper.find('#draft-title').setValue('外す指摘');
    await wrapper.find('#draft-body').setValue('本文');
    bodyButton('下書きに追加')!.click();
    await flushPromises();

    document.body
      .querySelector<HTMLButtonElement>('button[aria-label="下書きから「外す指摘」を外す"]')!
      .click();
    await flushPromises();

    expect(wrapper.find('[data-testid="staged-list"]').exists()).toBe(false);
  });

  it('確定に失敗したら理由を表示し、created を出さない', async () => {
    stubFetch(403);
    const wrapper = mountComposer();
    await flushPromises();

    await wrapper.find('#composer-head').setValue('abc');
    bodyButton('確定')!.click();
    await flushPromises();

    expect(wrapper.text()).toContain('レビューを起票する権限がありません。');
    expect(wrapper.emitted('created')).toBeUndefined();
  });
});
