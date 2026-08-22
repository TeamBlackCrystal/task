import { describe, it, expect, afterEach, vi } from 'vitest';
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils';
import { VueQueryPlugin, QueryClient } from '@tanstack/vue-query';
import IntegrationsSection from '../IntegrationsSection.vue';

const TENANT_UUID = '11111111-1111-1111-1111-111111111111';
const PROJECT_UUID = '00000000-0000-4000-8000-000000000010';
const INSTALL_URL = 'https://github.com/apps/test-app/installations/new?state=abc';

type MockState = {
  connected: boolean;
  /** 400 以上を設定すると GET /github/integration が失敗する */
  integrationStatus?: number;
  /** 400 以上を設定すると GET /github/install が失敗する */
  installStatus?: number;
  /** 400 以上を設定すると DELETE /github/integration が失敗する */
  deleteStatus?: number;
  /** true にすると DELETE /github/integration が解決せず、mutation が pending のままになる */
  hangDelete?: boolean;
  /** 400 以上を設定すると POST /github/import が失敗する */
  importStatus?: number;
  /** true にすると POST /github/import が解決せず、mutation が pending のままになる */
  hangImport?: boolean;
  /** 連携先リポジトリ名（差し替えると連携先が変わった状況を作れる） */
  repoName?: string;
};

const jsonResponse = (data: unknown, status = 200) =>
  new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });

function stubFetch(state: MockState) {
  const fetchMock = vi.fn(async (req: Request | string) => {
    const url = typeof req === 'string' ? req : req.url;
    const method = typeof req === 'string' ? 'GET' : req.method;
    const pathname = new URL(url, 'http://localhost').pathname;

    if (method === 'GET' && pathname.endsWith('/github/install')) {
      if (state.installStatus) return jsonResponse({ message: 'error' }, state.installStatus);
      return jsonResponse({ url: INSTALL_URL });
    }
    if (method === 'GET' && pathname.endsWith('/github/integration')) {
      if (state.integrationStatus)
        return jsonResponse({ message: 'error' }, state.integrationStatus);
      return jsonResponse(
        state.connected
          ? {
              connected: true,
              repo_owner: 'koyori-app',
              repo_name: state.repoName ?? 'koyori',
              connected_at: '2026-07-01T00:00:00Z',
            }
          : { connected: false, repo_owner: null, repo_name: null, connected_at: null },
      );
    }
    if (method === 'POST' && pathname.endsWith('/github/import')) {
      if (state.hangImport) return new Promise<Response>(() => {}); // 解決しない → isPending を保持
      if (state.importStatus) return jsonResponse({ message: 'error' }, state.importStatus);
      return new Response(null, { status: 202 });
    }
    if (method === 'DELETE' && pathname.endsWith('/github/integration')) {
      if (state.hangDelete) return new Promise<Response>(() => {}); // 解決しない → isPending を保持
      if (state.deleteStatus) return jsonResponse({ message: 'error' }, state.deleteStatus);
      state.connected = false;
      return new Response(null, { status: 204 });
    }
    return jsonResponse({ message: 'not-found' }, 404);
  });
  vi.stubGlobal('fetch', fetchMock);
  return fetchMock;
}

function mountSection() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const wrapper = mount(IntegrationsSection, {
    props: { tenantId: TENANT_UUID, projectId: PROJECT_UUID },
    global: { plugins: [[VueQueryPlugin, { queryClient }]] },
    attachTo: document.body,
  });
  return { wrapper, queryClient };
}

/**
 * 取り込み成功後のクールダウン（60 秒）を明けさせる。
 * 境界ちょうどで隠れるバグを避けるため、明ける側は 60 秒より後まで進める
 */
async function passImportCooldown() {
  await vi.advanceTimersByTimeAsync(61_000);
  await flushPromises();
}

function bodyButton(label: string) {
  return [...document.body.querySelectorAll('button')].find((b) => b.textContent?.trim() === label);
}

function clickBodyButton(label: string) {
  const button = bodyButton(label);
  if (!button) throw new Error(`button "${label}" not found`);
  button.click();
}

enableAutoUnmount(afterEach);

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('IntegrationsSection', () => {
  it('未連携なら GitHub カードと「連携する」ボタンを表示する', async () => {
    stubFetch({ connected: false });
    mountSection();
    await flushPromises();

    expect(document.body.textContent).toContain('GitHub');
    expect(document.body.textContent).toContain('コミットや Pull Request をタスクに紐付けます');
    expect(bodyButton('連携する')).toBeTruthy();
    expect(bodyButton('連携を解除')).toBeUndefined();
  });

  it('連携済みならリポジトリ名と「連携を解除」ボタンを表示する', async () => {
    stubFetch({ connected: true });
    mountSection();
    await flushPromises();

    expect(document.body.textContent).toContain('koyori-app/koyori');
    expect(document.body.textContent).toContain('を連携中');
    expect(bodyButton('連携を解除')).toBeTruthy();
    expect(bodyButton('連携する')).toBeUndefined();
  });

  it('連携済みなら「Issue を取り込む」で POST /github/import を呼び、開始を伝える', async () => {
    const fetchMock = stubFetch({ connected: true });
    mountSection();
    await flushPromises();

    clickBodyButton('Issue を取り込む');
    await flushPromises();

    const importCall = fetchMock.mock.calls
      .map(([req]) => req)
      .filter((req): req is Request => typeof req !== 'string')
      .find((req) => req.url.includes('/github/import'));
    expect(importCall).toBeTruthy();
    expect(importCall!.method).toBe('POST');
    expect(importCall!.url).toContain(`/tenants/${TENANT_UUID}/projects/${PROJECT_UUID}/`);
    expect(document.body.textContent).toContain('Issue の取り込みを開始しました');
  });

  it('取り込みの開始に失敗したらエラーを表示し、成功メッセージは消えて再度押せる', async () => {
    vi.useFakeTimers();
    const state: MockState = { connected: true };
    stubFetch(state);
    mountSection();
    await flushPromises();

    // 一度成功させてから失敗させ、前回の成功メッセージが残らないことを見る
    clickBodyButton('Issue を取り込む');
    await flushPromises();
    expect(document.body.textContent).toContain('Issue の取り込みを開始しました');

    await passImportCooldown();
    state.importStatus = 403;
    clickBodyButton('Issue を取り込む');
    await flushPromises();

    expect(document.body.textContent).toContain('Issue の取り込みを開始できませんでした');
    expect(document.body.textContent).not.toContain('Issue の取り込みを開始しました');
    expect(bodyButton('Issue を取り込む')?.disabled).toBe(false);
  });

  it('取り込みの開始中はボタンを押せなくする', async () => {
    stubFetch({ connected: true, hangImport: true });
    mountSection();
    await flushPromises();

    clickBodyButton('Issue を取り込む');
    await flushPromises();

    expect(bodyButton('Issue を取り込む')).toBeUndefined();
    expect(bodyButton('開始中…')?.disabled).toBe(true);
  });

  it('取り込みの開始に成功したら一定時間ボタンを押せなくする', async () => {
    vi.useFakeTimers();
    const fetchMock = stubFetch({ connected: true });
    mountSection();
    await flushPromises();

    clickBodyButton('Issue を取り込む');
    await flushPromises();

    const importCalls = () =>
      fetchMock.mock.calls
        .map(([req]) => req)
        .filter((req): req is Request => typeof req !== 'string')
        .filter((req) => req.url.includes('/github/import')).length;
    expect(importCalls()).toBe(1);

    // 成功直後はラベルが変わり、押しても POST が増えない
    expect(bodyButton('Issue を取り込む')).toBeUndefined();
    expect(bodyButton('取り込み中…')?.disabled).toBe(true);
    clickBodyButton('取り込み中…');
    await flushPromises();
    expect(importCalls()).toBe(1);

    // クールダウン中はまだ押せない
    await vi.advanceTimersByTimeAsync(59_000);
    await flushPromises();
    expect(bodyButton('取り込み中…')?.disabled).toBe(true);

    // 明けたら再び押せて、POST も届く
    await passImportCooldown();
    expect(bodyButton('取り込み中…')).toBeUndefined();
    expect(bodyButton('Issue を取り込む')?.disabled).toBe(false);
    clickBodyButton('Issue を取り込む');
    await flushPromises();
    expect(importCalls()).toBe(2);
  });

  it('取り込みの開始に失敗したらクールダウンを置かず、すぐ再試行できる', async () => {
    vi.useFakeTimers();
    const state: MockState = { connected: true, importStatus: 500 };
    stubFetch(state);
    mountSection();
    await flushPromises();

    clickBodyButton('Issue を取り込む');
    await flushPromises();

    expect(document.body.textContent).toContain('Issue の取り込みを開始できませんでした');
    expect(bodyButton('Issue を取り込む')?.disabled).toBe(false);
  });

  it('取り込み後に連携を解除したら取り込みの結果表示を残さない', async () => {
    vi.useFakeTimers();
    const state: MockState = { connected: true };
    stubFetch(state);
    mountSection();
    await flushPromises();

    clickBodyButton('Issue を取り込む');
    await flushPromises();
    expect(document.body.textContent).toContain('Issue の取り込みを開始しました');

    await passImportCooldown();
    state.importStatus = 500;
    clickBodyButton('Issue を取り込む');
    await flushPromises();
    expect(document.body.textContent).toContain('Issue の取り込みを開始できませんでした');

    clickBodyButton('連携を解除');
    await flushPromises();
    clickBodyButton('解除する');
    await flushPromises();
    await flushPromises();

    expect(bodyButton('連携する')).toBeTruthy();
    expect(document.body.textContent).not.toContain('Issue の取り込みを開始しました');
    expect(document.body.textContent).not.toContain('Issue の取り込みを開始できませんでした');
  });

  it('連携を解除したあと再連携しても取り込みの結果表示が戻らない', async () => {
    const state: MockState = { connected: true };
    stubFetch(state);
    const { queryClient } = mountSection();
    await flushPromises();

    clickBodyButton('Issue を取り込む');
    await flushPromises();
    expect(document.body.textContent).toContain('Issue の取り込みを開始しました');

    clickBodyButton('連携を解除');
    await flushPromises();
    clickBodyButton('解除する');
    await flushPromises();
    await flushPromises();
    expect(bodyButton('連携する')).toBeTruthy();

    // 別タブで再連携された状態を作り、この画面が再取得する（ウィンドウフォーカス相当）
    state.connected = true;
    await queryClient.refetchQueries();
    await flushPromises();

    expect(bodyButton('Issue を取り込む')).toBeTruthy();
    expect(document.body.textContent).not.toContain('Issue の取り込みを開始しました');
  });

  it('別タブで解除・再連携されても取り込みの結果表示とエラーが戻らない', async () => {
    // この画面では解除操作をしない。連携状態の変化だけで状態が捨てられることを見る
    const state: MockState = { connected: true, importStatus: 500 };
    stubFetch(state);
    const { queryClient } = mountSection();
    await flushPromises();

    clickBodyButton('Issue を取り込む');
    await flushPromises();
    expect(document.body.textContent).toContain('Issue の取り込みを開始できませんでした');

    // 別タブで解除された
    state.connected = false;
    await queryClient.refetchQueries();
    await flushPromises();
    expect(bodyButton('連携する')).toBeTruthy();

    // 別タブで再連携された
    state.connected = true;
    state.importStatus = undefined;
    await queryClient.refetchQueries();
    await flushPromises();

    expect(bodyButton('Issue を取り込む')).toBeTruthy();
    expect(document.body.textContent).not.toContain('Issue の取り込みを開始できませんでした');
  });

  it('連携先のリポジトリが変わったら取り込みの結果表示を引き継がない', async () => {
    const state: MockState = { connected: true };
    stubFetch(state);
    const { queryClient } = mountSection();
    await flushPromises();

    clickBodyButton('Issue を取り込む');
    await flushPromises();
    expect(document.body.textContent).toContain('Issue の取り込みを開始しました');

    state.repoName = 'another-repo';
    await queryClient.refetchQueries();
    await flushPromises();

    expect(document.body.textContent).toContain('koyori-app/another-repo');
    expect(document.body.textContent).not.toContain('Issue の取り込みを開始しました');
  });

  it('未連携なら「Issue を取り込む」を表示しない', async () => {
    stubFetch({ connected: false });
    mountSection();
    await flushPromises();

    expect(bodyButton('Issue を取り込む')).toBeUndefined();
  });

  it('「連携する」でインストール URL を取得して GitHub へ遷移する', async () => {
    const fetchMock = stubFetch({ connected: false });
    const assignSpy = vi.spyOn(window.location, 'assign').mockImplementation(() => {});
    mountSection();
    await flushPromises();

    clickBodyButton('連携する');
    await flushPromises();

    const installCall = fetchMock.mock.calls
      .map(([req]) => req)
      .filter((req): req is Request => typeof req !== 'string')
      .find((req) => req.url.includes('/github/install'));
    expect(installCall).toBeTruthy();
    expect(installCall!.url).toContain(`/tenants/${TENANT_UUID}/projects/${PROJECT_UUID}/`);
    expect(assignSpy).toHaveBeenCalledWith(INSTALL_URL);
  });

  it('インストール URL の取得に失敗したらエラーを表示する', async () => {
    stubFetch({ connected: false, installStatus: 500 });
    const assignSpy = vi.spyOn(window.location, 'assign').mockImplementation(() => {});
    mountSection();
    await flushPromises();

    clickBodyButton('連携する');
    await flushPromises();

    expect(document.body.textContent).toContain('GitHub のインストール URL を取得できませんでした');
    expect(assignSpy).not.toHaveBeenCalled();
    // 失敗後は再度押せる
    expect(bodyButton('連携する')?.disabled).toBe(false);
  });

  it('解除フロー: 確認ダイアログ → 解除する → DELETE 後に未連携表示へ戻る', async () => {
    const state: MockState = { connected: true };
    const fetchMock = stubFetch(state);
    mountSection();
    await flushPromises();

    clickBodyButton('連携を解除');
    await flushPromises();
    expect(document.body.textContent).toContain('GitHub 連携を解除しますか？');
    expect(document.body.textContent).toContain('「koyori-app/koyori」との連携を解除します。');

    clickBodyButton('解除する');
    await flushPromises();

    const deleteCall = fetchMock.mock.calls
      .map(([req]) => req)
      .filter((req): req is Request => typeof req !== 'string')
      .find((req) => req.method === 'DELETE');
    expect(deleteCall).toBeTruthy();
    expect(deleteCall!.url).toContain(
      `/tenants/${TENANT_UUID}/projects/${PROJECT_UUID}/github/integration`,
    );

    // invalidate による再取得後は未連携カードに戻り、ダイアログは閉じる
    await flushPromises();
    expect(document.body.textContent).not.toContain('GitHub 連携を解除しますか？');
    expect(bodyButton('連携する')).toBeTruthy();
  });

  it('解除に失敗したらダイアログ内にエラーを表示して開いたままにする', async () => {
    stubFetch({ connected: true, deleteStatus: 500 });
    mountSection();
    await flushPromises();

    clickBodyButton('連携を解除');
    await flushPromises();
    clickBodyButton('解除する');
    await flushPromises();

    expect(document.body.textContent).toContain('連携を解除できませんでした');
    expect(document.body.textContent).toContain('GitHub 連携を解除しますか？');
  });

  it('解除リクエスト進行中は確認ダイアログのクローズ要求を無視する', async () => {
    // DELETE を hang させ、mutation が pending の間に Esc/オーバーレイ相当の
    // update:open(false) を発火してもダイアログが閉じないことを検証する
    stubFetch({ connected: true, hangDelete: true });
    const { wrapper } = mountSection();
    await flushPromises();

    clickBodyButton('連携を解除');
    await flushPromises();
    expect(document.body.textContent).toContain('GitHub 連携を解除しますか？');

    // 解除を開始（DELETE は never-resolve なので isPending が true のまま）
    clickBodyButton('解除する');
    await flushPromises();

    const dialogRoot = wrapper.findComponent({ name: 'DialogRoot' });
    dialogRoot.vm.$emit('update:open', false);
    await flushPromises();

    expect(document.body.textContent).toContain('GitHub 連携を解除しますか？');
  });

  it('状態取得に失敗したらエラーと「再試行」を表示し、再試行で回復する', async () => {
    const state: MockState = { connected: false, integrationStatus: 500 };
    stubFetch(state);
    mountSection();
    await flushPromises();

    expect(document.body.textContent).toContain('連携状態を取得できませんでした');

    state.integrationStatus = undefined;
    clickBodyButton('再試行');
    await flushPromises();

    expect(document.body.textContent).not.toContain('連携状態を取得できませんでした');
    expect(bodyButton('連携する')).toBeTruthy();
  });

  it('API 未実装の Slack / Figma は描画しない', async () => {
    stubFetch({ connected: false });
    mountSection();
    await flushPromises();

    expect(document.body.textContent).not.toContain('Slack');
    expect(document.body.textContent).not.toContain('Figma');
  });
});
