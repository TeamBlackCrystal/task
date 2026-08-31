import { describe, it, expect, afterEach, vi } from 'vitest';
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils';
import { VueQueryPlugin, QueryClient } from '@tanstack/vue-query';
import ReviewFindingsView from '../ReviewFindingsView.vue';
import type { components } from '@/generated/api';

const TENANT_ID = '11111111-1111-1111-1111-111111111111';
const PROJECT_ID = '00000000-0000-4000-8000-000000000010';
const VIEWER_ID = '00000000-0000-0000-0000-0000000000aa';
const OTHER_ID = '00000000-0000-0000-0000-0000000000bb';

type Finding = components['schemas']['FindingResponse'];

function finding(overrides: Partial<Finding> = {}): Finding {
  return {
    id: 'f-1',
    review_id: 'r-1',
    pr_number: 618,
    round: 1,
    severity: 'high',
    title: '認可が抜けている',
    body: '再現条件と根拠',
    file: 'src/App.vue',
    line: 42,
    state: 'open',
    deferred_task_id: null,
    fixed_by: null,
    created_at: '2026-08-26T00:00:00Z',
    updated_at: '2026-08-26T00:00:00Z',
    transitions: [],
    ...overrides,
  };
}

type MockState = {
  findings: Finding[];
  blocking?: number;
  prsStatus?: number;
  patchStatus?: number;
  patchMessage?: string;
  /** ラウンドを出した人（取り下げを出してよいかの判定に使う） */
  roundReviewerId?: string;
  /** これまでのラウンド数（0 = 未レビュー） */
  rounds?: number;
  /** 集計対象のリポジトリ。null で「連携なし」を作る */
  repository?: string | null;
  /** 要約ジョブが確かめた現在の head。null で「鮮度不明」を作る */
  cachedHeadSha?: string | null;
  ownerOverrideRejections?: number;
};

const REVIEWED_HEAD = '60cdd7795f94fa4e4148ce996c2efb4c363e3f5e';

const jsonResponse = (data: unknown, status = 200) =>
  new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });

function stubFetch(state: MockState) {
  const patched: { path: string; body: unknown }[] = [];
  const fetchMock = vi.fn(async (req: Request) => {
    const pathname = new URL(req.url, 'http://localhost').pathname;

    if (req.method === 'GET' && pathname.endsWith('/reviews/pull-requests')) {
      if (state.prsStatus) return jsonResponse({ message: 'error' }, state.prsStatus);
      const blocking =
        state.blocking ??
        state.findings.filter(
          (f) =>
            (f.severity === 'high' || f.severity === 'medium') &&
            (f.state === 'open' || f.state === 'fixed'),
        ).length;
      return jsonResponse([
        {
          pr_number: 618,
          rounds: state.rounds ?? 1,
          pr_title: 'feat: レビュー指摘管理',
          pr_author: 'yupix',
          unresolved: state.findings.filter((f) => f.state === 'open' || f.state === 'fixed')
            .length,
          blocking,
          last_reviewed_at: '2026-08-26T00:00:00Z',
        },
      ]);
    }
    if (req.method === 'GET' && pathname.endsWith('/reviews/summary')) {
      const blocking =
        state.blocking ??
        state.findings.filter(
          (f) =>
            (f.severity === 'high' || f.severity === 'medium') &&
            (f.state === 'open' || f.state === 'fixed'),
        ).length;
      const rounds = state.rounds ?? 1;
      return jsonResponse({
        pr_number: 618,
        rounds,
        counts: state.findings.map((f) => ({
          severity: f.severity,
          state: f.state,
          count: 1,
        })),
        blocking,
        latest_head_sha: rounds > 0 ? REVIEWED_HEAD : null,
        // 既定は「連携あり・レビューした commit が現在の head」＝可を出してよい状態
        repository: state.repository === undefined ? 'acme/app' : state.repository,
        cached_pr_head_sha: state.cachedHeadSha === undefined ? REVIEWED_HEAD : state.cachedHeadSha,
        pr_head_checked_at: '2026-08-28T10:00:00Z',
        owner_override_rejections: state.ownerOverrideRejections ?? 0,
        mergeable: rounds > 0 && blocking === 0,
      });
    }
    if (req.method === 'GET' && pathname.endsWith('/reviews')) {
      return jsonResponse([
        {
          id: 'r-1',
          project_id: PROJECT_ID,
          pr_number: 618,
          round: 1,
          head_sha: '60cdd7795f94',
          reviewer: {
            id: state.roundReviewerId ?? OTHER_ID,
            username: 'reviewer',
            avatar_url: null,
          },
          summary: '総評',
          pr_title: null,
          pr_author: null,
          created_at: '2026-08-26T00:00:00Z',
          finding_count: state.findings.length,
        },
      ]);
    }
    if (req.method === 'GET' && pathname.endsWith('/review-findings')) {
      return jsonResponse(state.findings);
    }
    if (req.method === 'PATCH' && pathname.includes('/review-findings/')) {
      const body = await req.clone().json();
      patched.push({ path: pathname, body });
      if (state.patchStatus)
        return jsonResponse({ message: state.patchMessage ?? 'error' }, state.patchStatus);
      const id = pathname.split('/').pop();
      state.findings = state.findings.map((f) =>
        f.id === id ? { ...f, state: (body as { state: Finding['state'] }).state } : f,
      );
      return jsonResponse(state.findings.find((f) => f.id === id));
    }
    return jsonResponse({ message: 'not-found' }, 404);
  });
  vi.stubGlobal('fetch', fetchMock);
  return { patched };
}

function mountView() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return mount(ReviewFindingsView, {
    props: {
      tenantId: TENANT_ID,
      tenantSlug: 'acme',
      projectId: PROJECT_ID,
      projectKey: 'APP',
      viewerId: VIEWER_ID,
      initialPr: 618,
    },
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

describe('ReviewFindingsView', () => {
  it('指摘とマージ判定を表示する', async () => {
    stubFetch({ findings: [finding()] });
    const wrapper = mountView();
    await flushPromises();

    const gate = wrapper.get('[data-testid="merge-gate"]');
    expect(gate.text()).toContain('マージ不可');
    const list = wrapper.get('[data-testid="finding-list"]');
    expect(list.text()).toContain('認可が抜けている');
    expect(list.text()).toContain('src/App.vue:42');
    expect(list.text()).toContain('High');
  });

  it('未解決が無ければマージ可を出す', async () => {
    stubFetch({ findings: [finding({ state: 'verified' })] });
    const wrapper = mountView();
    await flushPromises();

    expect(wrapper.get('[data-testid="merge-gate"]').text()).toContain('マージ可');
  });

  it('状態遷移を送り、一覧に反映する', async () => {
    const { patched } = stubFetch({ findings: [finding()] });
    const wrapper = mountView();
    await flushPromises();

    bodyButton('修正した')!.click();
    await flushPromises();

    expect(patched).toHaveLength(1);
    expect(patched[0].body).toEqual({ state: 'fixed', note: null });
    expect(patched[0].path.endsWith('/review-findings/f-1')).toBe(true);
    expect(wrapper.get('[data-testid="finding-list"]').text()).toContain('Fixed');
  });

  it('自分で fixed を宣言した指摘は確認ボタンを押せない', async () => {
    // 確認と差し戻しはレビュー側だけに出るので、閲覧者をラウンドの作成者にしておく
    stubFetch({
      findings: [finding({ state: 'fixed', fixed_by: VIEWER_ID })],
      roundReviewerId: VIEWER_ID,
    });
    const wrapper = mountView();
    await flushPromises();

    expect(bodyButton('確認した')?.disabled).toBe(true);
    expect(wrapper.text()).toContain('修正者と確認者は別の人である必要があります');
    // 差し戻しは押せる
    expect(bodyButton('レビューに戻す')?.disabled).toBe(false);
  });

  it('別の人が直した指摘はレビュー側なら確認できる', async () => {
    stubFetch({
      findings: [finding({ state: 'fixed', fixed_by: OTHER_ID })],
      roundReviewerId: VIEWER_ID,
    });
    mountView();
    await flushPromises();

    expect(bodyButton('確認した')?.disabled).toBe(false);
  });

  it('レビュー側でない人に確認・差し戻しのボタンを出さない（サーバーも 403 で拒否する）', async () => {
    // ラウンドを 1 本も出していない＝修正だけを行う利用者。この画面の主要な利用者で、
    // 同僚が宣言した fixed に確認ボタンを出すと押した瞬間に 403 になる
    stubFetch({
      findings: [finding({ state: 'fixed', fixed_by: OTHER_ID })],
      roundReviewerId: OTHER_ID,
    });
    mountView();
    await flushPromises();

    expect(bodyButton('確認した')).toBeUndefined();
    expect(bodyButton('レビューに戻す')).toBeUndefined();
  });

  it('High には繰り延べのボタンを出さない（サーバーも 409 で拒否する）', async () => {
    stubFetch({ findings: [finding({ severity: 'high' })] });
    mountView();
    await flushPromises();

    expect(bodyButton('繰り延べる')).toBeUndefined();
    // 他の操作は出る（繰り延べだけを落としている）
    expect(bodyButton('修正した')?.disabled).toBe(false);
  });

  it('Low には繰り延べのボタンを出す', async () => {
    stubFetch({ findings: [finding({ severity: 'low' })] });
    mountView();
    await flushPromises();

    expect(bodyButton('繰り延べる')?.disabled).toBe(false);
  });

  it('他人が出した指摘には取り下げのボタンを出さない（サーバーも 403 で拒否する）', async () => {
    stubFetch({ findings: [finding({ severity: 'low' })] });
    mountView();
    await flushPromises();

    expect(bodyButton('指摘を取り下げる')).toBeUndefined();
    // 修正の宣言と繰り延べは出る（取り下げだけを落としている）
    expect(bodyButton('修正した')?.disabled).toBe(false);
    expect(bodyButton('繰り延べる')?.disabled).toBe(false);
  });

  it('自分が出した指摘には取り下げのボタンを出す', async () => {
    stubFetch({ findings: [finding({ severity: 'low' })], roundReviewerId: VIEWER_ID });
    mountView();
    await flushPromises();

    expect(bodyButton('指摘を取り下げる')?.disabled).toBe(false);
  });

  it('一覧バッジは件数だけを出し、可否を断定しない', async () => {
    // 未解決ゼロでも「マージ可」ではなく「未解決なし」。可否には鮮度と連携の
    // 有無も要り、一覧はその材料（latest_head_sha / cached_pr_head_sha）を持たない
    stubFetch({ findings: [] });
    const wrapper = mountView();
    await flushPromises();
    expect(wrapper.text()).toContain('未解決なし');

    // 未解決があれば件数
    stubFetch({ findings: [finding({ severity: 'high' })] });
    const blocked = mountView();
    await flushPromises();
    expect(blocked.text()).toContain('1 件が未解決');
  });

  it('未レビューの PR は「マージ可」と言わない', async () => {
    stubFetch({ findings: [], rounds: 0 });
    const wrapper = mountView();
    await flushPromises();

    const gate = wrapper.get('[data-testid="merge-gate"]').text();
    expect(gate).toContain('未レビュー');
    // パネルだけでなく画面全体で見る。一覧バッジが可否を断定すると、
    // パネルが降格させた横で緑の「マージ可」が出る矛盾になる
    expect(wrapper.text()).not.toContain('マージ可');
  });

  it('レビュー済みならレビューした commit を出す（鮮度を目で確かめられる）', async () => {
    stubFetch({ findings: [] });
    const wrapper = mountView();
    await flushPromises();

    expect(wrapper.text()).toContain('マージ可');
    expect(wrapper.text()).toContain(REVIEWED_HEAD.slice(0, 7));
  });

  it('連携が無ければ「マージ可」を出さない', async () => {
    stubFetch({ findings: [], repository: null });
    const wrapper = mountView();
    await flushPromises();

    const gate = wrapper.get('[data-testid="merge-gate"]').text();
    expect(gate).toContain('リポジトリ未確定');
    expect(wrapper.text()).not.toContain('マージ可');
  });

  it('レビュー後にコミットが積まれていれば「マージ可」を出さない', async () => {
    stubFetch({ findings: [], cachedHeadSha: 'ffffffffffffffffffffffffffffffffffffffff' });
    const wrapper = mountView();
    await flushPromises();

    const gate = wrapper.get('[data-testid="merge-gate"]').text();
    expect(gate).toContain('レビューが古い');
    expect(wrapper.text()).not.toContain('マージ可');
  });

  it('現在の HEAD を確かめられていなければ「マージ可」を出さない', async () => {
    stubFetch({ findings: [], cachedHeadSha: null });
    const wrapper = mountView();
    await flushPromises();

    const gate = wrapper.get('[data-testid="merge-gate"]').text();
    expect(gate).toContain('鮮度不明');
    expect(wrapper.text()).not.toContain('マージ可');
  });

  it('オーナー代行での棄却は件数を出す', async () => {
    stubFetch({ findings: [], ownerOverrideRejections: 2 });
    const wrapper = mountView();
    await flushPromises();

    expect(wrapper.get('[data-testid="merge-gate"]').text()).toContain('オーナー代行での棄却 2 件');
  });

  it('verified の指摘には操作ボタンを出さない（終端）', async () => {
    stubFetch({ findings: [finding({ state: 'verified' })] });
    mountView();
    await flushPromises();

    for (const label of ['修正した', '確認した', '繰り延べる', '再オープン']) {
      expect(bodyButton(label), label).toBeUndefined();
    }
  });

  it('サーバーが理由を返した 409 はその文言を出す', async () => {
    stubFetch({
      findings: [finding({ severity: 'low' })],
      patchStatus: 409,
      patchMessage: 'high の指摘は繰り延べられません（繰り延べは low / nit のみ）',
    });
    const wrapper = mountView();
    await flushPromises();

    bodyButton('繰り延べる')!.click();
    await flushPromises();

    expect(wrapper.text()).toContain('繰り延べは low / nit のみ');
  });

  it('スラグだけの本文は出さず、状態に応じた説明に落とす', async () => {
    stubFetch({
      findings: [finding({ severity: 'low' })],
      patchStatus: 409,
      patchMessage: 'conflict',
    });
    const wrapper = mountView();
    await flushPromises();

    bodyButton('繰り延べる')!.click();
    await flushPromises();

    expect(wrapper.text()).toContain('いまの状態からは行えない操作です');
    expect(wrapper.text()).not.toContain('conflict');
  });

  it('403 のときは理由を表示する', async () => {
    stubFetch({
      findings: [finding({ state: 'fixed', fixed_by: OTHER_ID })],
      roundReviewerId: VIEWER_ID,
      patchStatus: 403,
    });
    const wrapper = mountView();
    await flushPromises();

    bodyButton('確認した')!.click();
    await flushPromises();

    expect(wrapper.text()).toContain('この操作はレビュー側だけが行えます');
  });

  it('409 のときは再読み込みを促す', async () => {
    stubFetch({ findings: [finding()], patchStatus: 409 });
    const wrapper = mountView();
    await flushPromises();

    bodyButton('修正した')!.click();
    await flushPromises();

    expect(wrapper.text()).toContain('いまの状態からは行えない操作です');
  });

  it('読み込みに失敗したらエラーを表示する', async () => {
    stubFetch({ findings: [], prsStatus: 500 });
    const wrapper = mountView();
    await flushPromises();

    expect(wrapper.text()).toContain('レビューを読み込めませんでした');
  });
});
