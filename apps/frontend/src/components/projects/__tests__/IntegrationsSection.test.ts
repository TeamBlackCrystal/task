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
  /** 400 以上を設定すると GET /github/repositories が失敗する（選択トークンの期限切れ相当） */
  repositoriesStatus?: number;
  /** 400 以上を設定すると POST /github/connect が失敗する */
  connectStatus?: number;
  /** GET /github/repositories が返す一覧（省略時は DEFAULT_REPOSITORIES） */
  repositories?: { owner: string; name: string }[];
  /** 400 以上を設定すると POST /github/import が失敗する */
  importStatus?: number;
  /** true にすると POST /github/import が解決せず、mutation が pending のままになる */
  hangImport?: boolean;
  /** 連携先リポジトリ名（差し替えると連携先が変わった状況を作れる） */
  repoName?: string;
};

const DEFAULT_REPOSITORIES = [
  { owner: 'koyori-app', name: 'koyori' },
  { owner: 'koyori-app', name: 'docs' },
];

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
    if (method === 'GET' && pathname.endsWith('/github/repositories')) {
      if (state.repositoriesStatus)
        return jsonResponse({ message: 'error' }, state.repositoriesStatus);
      return jsonResponse({ repositories: state.repositories ?? DEFAULT_REPOSITORIES });
    }
    if (method === 'POST' && pathname.endsWith('/github/connect')) {
      if (state.connectStatus) return jsonResponse({ message: 'error' }, state.connectStatus);
      state.connected = true;
      return new Response(null, { status: 204 });
    }
    if (method === 'POST' && pathname.endsWith('/github/import')) {
      if (state.hangImport) return new Promise<Response>(() => {}); // 解決しない → isPending を保持
      if (state.importStatus)
        return jsonResponse(
          { message: state.importStatus === 409 ? 'conflict' : 'error' },
          state.importStatus,
        );
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

function mountSection(options: { selectToken?: string; callbackError?: string } = {}) {
  // callback からの戻りは URL で表現される。選択トークンだけはフラグメント
  // （クエリだとアクセスログ・Referer に残るため）。
  const search = new URLSearchParams();
  if (options.callbackError !== undefined) search.set('github_error', options.callbackError);
  const hash = new URLSearchParams();
  if (options.selectToken !== undefined) hash.set('github_select', options.selectToken);
  const query = search.toString();
  const fragment = hash.toString();
  window.history.replaceState(
    {},
    '',
    `/settings${query ? `?${query}` : ''}${fragment ? `#${fragment}` : ''}`,
  );

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

function clickSelectButton(index: number) {
  const buttons = [...document.body.querySelectorAll('button')].filter(
    (b) => b.textContent?.trim() === '選択',
  );
  const button = buttons[index];
  if (!button) throw new Error(`select button #${index} not found`);
  button.click();
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

  it('取り込みが既に実行中なら専用のメッセージを表示する', async () => {
    stubFetch({ connected: true, importStatus: 409 });
    mountSection();
    await flushPromises();

    clickBodyButton('Issue を取り込む');
    await flushPromises();

    expect(document.body.textContent).toContain('Issue の取り込みは既に実行中です');
    expect(document.body.textContent).not.toContain('Issue の取り込みを開始できませんでした');
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

  it('選択トークン付きで戻ってきたらリポジトリ一覧を出し、選んだ 1 件を連携する', async () => {
    const fetchMock = stubFetch({ connected: false });
    mountSection({ selectToken: 'select-token-1' });
    await flushPromises();

    expect(document.body.textContent).toContain('連携するリポジトリを選択');
    expect(document.body.textContent).toContain('koyori-app/docs');

    const listCall = fetchMock.mock.calls
      .map(([req]) => req)
      .filter((req): req is Request => typeof req !== 'string')
      .find((req) => req.url.includes('/github/repositories'));
    // トークンはクエリではなくヘッダーで送る（クエリだと backend / プロキシの
    // アクセスログに残り、フラグメントで渡した意味が無くなる）。
    expect(listCall!.url).not.toContain('select_token');
    expect(listCall!.headers.get('X-Github-Select-Token')).toBe('select-token-1');

    // 2 件目（koyori-app/docs）の「選択」を押す
    const buttons = [...document.body.querySelectorAll('button')].filter(
      (b) => b.textContent?.trim() === '選択',
    );
    expect(buttons).toHaveLength(2);
    buttons[1]!.click();
    await flushPromises();

    const connectCall = fetchMock.mock.calls
      .map(([req]) => req)
      .filter((req): req is Request => typeof req !== 'string')
      .find((req) => req.url.includes('/github/connect'));
    expect(connectCall).toBeTruthy();
    await expect(connectCall!.clone().json()).resolves.toEqual({
      select_token: 'select-token-1',
      repo_owner: 'koyori-app',
      repo_name: 'docs',
    });

    await flushPromises();
    expect(document.body.textContent).not.toContain('連携するリポジトリを選択');
    expect(document.body.textContent).toContain('koyori-app/koyori');
  });

  it('選択トークンが切れていたら理由を出し、未連携表示に戻る', async () => {
    stubFetch({ connected: false, repositoriesStatus: 400 });
    mountSection({ selectToken: 'expired-token' });
    await flushPromises();

    expect(document.body.textContent).toContain('選択の有効期限が切れました');
    // 期限切れに再試行は無意味なのでボタンは出さない
    expect(bodyButton('再試行')).toBeUndefined();
    expect(bodyButton('連携する')).toBeTruthy();
  });

  it('連携が 4xx でもトークンが生きていれば一覧を取り直して選び直させる', async () => {
    stubFetch({ connected: false, connectStatus: 400 });
    mountSection({ selectToken: 'select-token-1' });
    await flushPromises();

    clickSelectButton(0);
    await flushPromises();

    expect(document.body.textContent).toContain('このリポジトリは選べませんでした');
    expect(document.body.textContent).toContain('koyori-app/koyori');
  });

  it('連携が 4xx でトークンも切れていたら期限切れとして畳む', async () => {
    const state: MockState = { connected: false, connectStatus: 400 };
    stubFetch(state);
    mountSection({ selectToken: 'select-token-1' });
    await flushPromises();

    // 連携要求と同時にトークンが失効した状況
    state.repositoriesStatus = 400;
    clickSelectButton(0);
    await flushPromises();

    expect(document.body.textContent).toContain('選択の有効期限が切れました');
    expect(bodyButton('連携する')).toBeTruthy();
  });

  it('連携が 5xx なら選択 UI を残してエラーを表示する', async () => {
    stubFetch({ connected: false, connectStatus: 500 });
    mountSection({ selectToken: 'select-token-1' });
    await flushPromises();

    clickSelectButton(0);
    await flushPromises();

    expect(document.body.textContent).toContain('リポジトリを連携できませんでした');
    expect(document.body.textContent).toContain('koyori-app/koyori');
  });

  it('一覧取得が 5xx なら選択 UI を残し、再試行で回復する', async () => {
    const state: MockState = { connected: false, repositoriesStatus: 500 };
    stubFetch(state);
    mountSection({ selectToken: 'select-token-1' });
    await flushPromises();

    expect(document.body.textContent).toContain('リポジトリ一覧を取得できませんでした');
    expect(document.body.textContent).toContain('連携するリポジトリを選択');
    // 取得失敗を「0 件」と取り違えさせない
    expect(document.body.textContent).not.toContain('選択できるリポジトリがありません');

    state.repositoriesStatus = undefined;
    clickBodyButton('再試行');
    await flushPromises();

    expect(document.body.textContent).toContain('koyori-app/docs');
  });

  it('所有者確認に落ちた場合は、アンインストールではなく入れ直しを促す', async () => {
    stubFetch({ connected: false });
    mountSection({ callbackError: 'installation_forbidden' });
    await flushPromises();

    expect(document.body.textContent).toContain('あなたのアカウントからは操作できません');
    // 一時障害やアンインストール案内と取り違えない
    expect(document.body.textContent).not.toContain('一度アンインストール');
    expect(bodyButton('連携する')).toBeTruthy();
  });

  it('ユーザー認可が無効な App では、入れ直しではなく設定の確認を促す', async () => {
    stubFetch({ connected: false });
    mountSection({ callbackError: 'installation_authorization_required' });
    await flushPromises();

    expect(document.body.textContent).toContain('管理者に設定の確認を依頼してください');
    // 所有者違い（入れ直しで直る）と取り違えない
    expect(document.body.textContent).not.toContain('もう一度インストールしてください');
  });

  it('リポジトリが多いときは入力欄で絞り込める', async () => {
    stubFetch({
      connected: false,
      repositories: [
        { owner: 'koyori-app', name: 'koyori' },
        { owner: 'koyori-app', name: 'docs' },
        { owner: 'other-org', name: 'infra' },
      ],
    });
    mountSection({ selectToken: 'select-token-1' });
    await flushPromises();

    const filter = document.body.querySelector<HTMLInputElement>(
      'input[aria-label="リポジトリを絞り込む"]',
    );
    expect(filter).toBeTruthy();

    filter!.value = 'other-org/inf';
    filter!.dispatchEvent(new Event('input'));
    await flushPromises();

    expect(document.body.textContent).toContain('other-org/infra');
    expect(document.body.textContent).not.toContain('koyori-app/docs');

    // 一致しないときは空リストではなく理由を出す
    filter!.value = 'no-such-repo';
    filter!.dispatchEvent(new Event('input'));
    await flushPromises();
    expect(document.body.textContent).toContain('一致するリポジトリはありません');
    expect(bodyButton('選択')).toBeUndefined();
  });

  it('リポジトリ 0 件で戻された場合は理由を表示する', async () => {
    stubFetch({ connected: false });
    mountSection({ callbackError: 'no_repositories' });
    await flushPromises();

    expect(document.body.textContent).toContain('リポジトリが 1 件も含まれていません');
    expect(bodyButton('連携する')).toBeTruthy();
  });

  it('連携後にセクションを開き直しても、消費済みトークンで再取得しない（退避も消す）', async () => {
    const fetchMock = stubFetch({ connected: false });
    const { wrapper: first } = mountSection({ selectToken: 'select-token-1' });
    await flushPromises();
    clickSelectButton(0);
    await flushPromises();
    first.unmount();

    // 再マウント（セクション切り替え相当）。URL からトークンは落ちている
    expect(window.location.hash).not.toContain('github_select');
    fetchMock.mockClear();
    mountSection();
    await flushPromises();

    const refetched = fetchMock.mock.calls
      .map(([req]) => req)
      .filter((req): req is Request => typeof req !== 'string')
      .some((req) => req.url.includes('/github/repositories'));
    expect(refetched).toBe(false);
    expect(document.body.textContent).not.toContain('選択の有効期限が切れました');
  });

  it('セクションを開き直しても、選択中のトークンは失われない', async () => {
    stubFetch({ connected: false });
    const { wrapper: first } = mountSection({ selectToken: 'select-token-1' });
    await flushPromises();
    expect(document.body.textContent).toContain('koyori-app/docs');
    first.unmount();

    // 再マウント（セクション切り替え相当）。URL にトークンは無いが選択は続けられる
    mountSection();
    await flushPromises();
    expect(document.body.textContent).toContain('koyori-app/docs');

    clickSelectButton(0);
    await flushPromises();
    expect(document.body.textContent).toContain('koyori-app/koyori');
  });

  it('セッション切れ（401）では選択トークンを捨てない', async () => {
    const state: MockState = { connected: false, repositoriesStatus: 401 };
    stubFetch(state);
    mountSection({ selectToken: 'select-token-1' });
    await flushPromises();

    expect(document.body.textContent).not.toContain('選択の有効期限が切れました');
    expect(document.body.textContent).toContain('リポジトリ一覧を取得できませんでした');

    state.repositoriesStatus = undefined;
    clickBodyButton('再試行');
    await flushPromises();
    expect(document.body.textContent).toContain('koyori-app/docs');
  });
});
