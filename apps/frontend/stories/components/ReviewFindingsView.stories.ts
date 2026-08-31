import type { Meta, StoryObj } from '@storybook/vue3-vite';
import { expect, fn, userEvent, waitFor, within } from 'storybook/test';
import { provide } from 'vue';
import { QueryClient, VUE_QUERY_CLIENT } from '@tanstack/vue-query';

import ReviewFindingsView from '@/components/reviews/ReviewFindingsView.vue';

const TENANT_UUID = '11111111-1111-1111-1111-111111111111';
const PROJECT_UUID = '00000000-0000-4000-8000-000000000010';
/** レビューした commit ＝ 現在の HEAD（鮮度は満たしている状態）。 */
const LATEST_HEAD_SHA = 'be31c05f9a1244d3b0e6f7a8c9d0e1f2a3b4c5d6';
const VIEWER_ID = '00000000-0000-0000-0000-0000000000aa';
const OTHER_ID = '00000000-0000-0000-0000-0000000000bb';

type StoryFinding = {
  id: string;
  severity: 'high' | 'medium' | 'low' | 'nit';
  state: 'open' | 'fixed' | 'verified' | 'deferred' | 'rejected';
  title: string;
  body: string;
  file: string | null;
  line: number | null;
  round: number;
  fixed_by: string | null;
};

const sampleFindings: StoryFinding[] = [
  {
    id: 'f-118',
    severity: 'high',
    state: 'open',
    title: '選択状態が URL に反映されず、リロードで詳細ペインが閉じる',
    body: '行クリックで selectedId をローカル state に持っているだけなので、リロードや共有リンクで詳細が復元されません。',
    file: 'src/pages/tasks/SplitView.vue',
    line: 142,
    round: 2,
    fixed_by: null,
  },
  {
    id: 'f-117',
    severity: 'high',
    state: 'fixed',
    title: '幅リサイズのイベントリスナが解除されない',
    body: 'mousemove / mouseup を document に登録していますが、unmount 時の解除がありません。',
    file: 'src/pages/tasks/SplitView.vue',
    line: 204,
    round: 2,
    // 閲覧者自身が直した指摘。自分では確認できない
    fixed_by: VIEWER_ID,
  },
  {
    id: 'f-116',
    severity: 'low',
    state: 'deferred',
    title: 'ステータスバッジの幅が揃わず列がずれる',
    body: '繰り延べて通常タスクに送りました。',
    file: 'src/components/StatusBadge.vue',
    line: null,
    round: 1,
    fixed_by: null,
  },
];

const jsonResponse = (data: unknown, status = 200) =>
  new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });

let fetchSpy: ReturnType<typeof fn> | null = null;

function toResponse(finding: StoryFinding) {
  return {
    id: finding.id,
    review_id: `r-${finding.round}`,
    pr_number: 412,
    round: finding.round,
    severity: finding.severity,
    title: finding.title,
    body: finding.body,
    file: finding.file,
    line: finding.line,
    state: finding.state,
    deferred_task_id: finding.state === 'deferred' ? 'task-1' : null,
    fixed_by: finding.fixed_by,
    created_at: '2026-08-25T10:12:00Z',
    updated_at: '2026-08-25T14:03:00Z',
    transitions: [
      {
        id: `t-${finding.id}`,
        actor: { id: OTHER_ID, username: 'reviewer', avatar_url: null },
        from_state: null,
        to_state: 'open',
        note: '9f4e7b2 を見て指摘',
        created_at: '2026-08-25T10:12:00Z',
      },
    ],
  };
}

/** レビュー系 API をインメモリの配列で応答する fetch モック */
function mockFetch(overrides: { empty?: boolean } = {}) {
  return () => {
    const original = globalThis.fetch;
    let findings = overrides.empty ? [] : sampleFindings.map((f) => ({ ...f }));

    const blocking = () =>
      findings.filter(
        (f) =>
          (f.severity === 'high' || f.severity === 'medium') &&
          (f.state === 'open' || f.state === 'fixed'),
      ).length;

    fetchSpy = fn().mockImplementation(async (req: Request | string) => {
      const url = typeof req === 'string' ? req : req.url;
      const method = typeof req === 'string' ? 'GET' : req.method;
      const pathname = new URL(url, 'http://localhost').pathname;

      if (method === 'GET' && pathname.endsWith('/reviews/pull-requests')) {
        if (findings.length === 0 && overrides.empty) return jsonResponse([]);
        return jsonResponse([
          {
            pr_number: 412,
            rounds: 2,
            pr_title: 'feat(frontend): タスク一覧の分割ビュー',
            pr_author: 'shadcn',
            unresolved: findings.filter((f) => f.state === 'open' || f.state === 'fixed').length,
            blocking: blocking(),
            last_reviewed_at: '2026-08-25T10:12:00Z',
          },
        ]);
      }
      if (method === 'GET' && pathname.endsWith('/reviews/summary')) {
        return jsonResponse({
          pr_number: 412,
          rounds: 2,
          counts: findings.map((f) => ({ severity: f.severity, state: f.state, count: 1 })),
          blocking: blocking(),
          mergeable: blocking() === 0,
          // repository が無いと mergeVerdict は「リポジトリ未確定」へ降格する。
          // 集計の視界が連携先で決まる以上、それが最初の判定になるので、
          // 件数や鮮度の見え方を確かめたいストーリーでは必ず埋める
          repository: 'koyori-app/task',
          owner_override_rejections: 0,
          // レビューした commit と現在の HEAD。揃えて「鮮度は満たしている」状態にし、
          // 未解決の有無だけが判定に効くようにする
          latest_head_sha: LATEST_HEAD_SHA,
          cached_pr_head_sha: LATEST_HEAD_SHA,
          pr_head_checked_at: '2026-08-25T10:12:00Z',
        });
      }
      if (method === 'GET' && pathname.endsWith('/reviews')) {
        return jsonResponse(
          [2, 1].map((round) => ({
            id: `r-${round}`,
            project_id: PROJECT_UUID,
            pr_number: 412,
            round,
            // API は 40 桁の小文字 16 進しか受け付けない。短縮 SHA をモックに置くと、
            // 実データでは起きない見え方（表示側の slice 前提など）を通してしまう
            head_sha: round === 2 ? LATEST_HEAD_SHA : '77bd214c8e0198a7b6c5d4e3f2a1b0c9d8e7f6a5',
            // 最新ラウンドは閲覧者が出したことにする。確認（verified）と差し戻しは
            // レビュー側にしか出ないので、reviewer を全部他人にすると
            // 「自分の修正は自分で確認できない」を見せる前にボタンが消える
            reviewer:
              round === 2
                ? { id: VIEWER_ID, username: 'viewer', avatar_url: null }
                : { id: OTHER_ID, username: 'reviewer', avatar_url: null },
            summary: '総評',
            pr_title: null,
            pr_author: null,
            created_at: '2026-08-25T10:12:00Z',
            finding_count: findings.filter((f) => f.round === round).length,
          })),
        );
      }
      if (method === 'GET' && pathname.endsWith('/review-findings')) {
        return jsonResponse(findings.map(toResponse));
      }
      if (method === 'PATCH' && pathname.includes('/review-findings/')) {
        const body = await (req as Request).json();
        const id = pathname.split('/').pop();
        findings = findings.map((f) => (f.id === id ? { ...f, state: body.state } : f));
        const updated = findings.find((f) => f.id === id)!;
        return jsonResponse(toResponse(updated));
      }
      if (method === 'POST' && pathname.endsWith('/reviews')) {
        return jsonResponse({ id: 'r-3', round: 3 }, 201);
      }
      return jsonResponse({ message: 'not-found' }, 404);
    });
    globalThis.fetch = fetchSpy;
    return () => {
      globalThis.fetch = original;
      fetchSpy = null;
    };
  };
}

function storyDecorator() {
  return () => ({
    setup() {
      const queryClient = new QueryClient({
        defaultOptions: {
          queries: { retry: false, gcTime: 0, staleTime: 0 },
          mutations: { retry: false },
        },
      });
      provide(VUE_QUERY_CLIENT, queryClient);
    },
    template: '<story />',
  });
}

const meta = {
  title: 'Components/Reviews/ReviewFindingsView',
  component: ReviewFindingsView,
  tags: ['autodocs'],
  args: {
    tenantId: TENANT_UUID,
    tenantSlug: 'acme',
    projectId: PROJECT_UUID,
    projectKey: 'APP',
    viewerId: VIEWER_ID,
    initialPr: 412,
  },
  parameters: {
    layout: 'padded',
    docs: {
      description: {
        component:
          'レビュー指摘の画面。PR 一覧・マージ判定・Round / 重大度 / 状態の絞り込み・状態遷移・ラウンドの起票（下書き → 一括確定）。fetch モックで検証。',
      },
    },
  },
  decorators: [storyDecorator()],
} satisfies Meta<typeof ReviewFindingsView>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  name: '一覧表示（マージ判定つき）',
  beforeEach: mockFetch(),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(
      canvas.findByRole('heading', { name: 'レビュー指摘' }),
    ).resolves.toBeInTheDocument();
    await expect(
      canvas.findByText('選択状態が URL に反映されず、リロードで詳細ペインが閉じる'),
    ).resolves.toBeInTheDocument();
    // High が 2 件未解決なのでマージ不可
    await expect(canvas.findByTestId('merge-gate')).resolves.toHaveTextContent('マージ不可');
  },
};

export const Empty: Story = {
  name: '空状態',
  beforeEach: mockFetch({ empty: true }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.findByText('レビューはまだありません。')).resolves.toBeInTheDocument();
  },
};

export const SelfVerificationBlocked: Story = {
  name: '自分の修正は自分で確認できない',
  beforeEach: mockFetch(),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await canvas.findByText('幅リサイズのイベントリスナが解除されない');

    // 閲覧者自身が fixed を宣言した指摘なので、確認ボタンは押せない
    await expect(canvas.getByRole('button', { name: '確認した' })).toBeDisabled();
    await expect(
      canvas.getByText('修正者と確認者は別の人である必要があります'),
    ).toBeInTheDocument();
  },
};

export const TransitionFlow: Story = {
  name: '状態遷移（修正した → 一覧に反映）',
  beforeEach: mockFetch(),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const user = userEvent.setup();
    await canvas.findByText('選択状態が URL に反映されず、リロードで詳細ペインが閉じる');

    await user.click(canvas.getByRole('button', { name: '修正した' }));

    await waitFor(() => expect(canvas.getAllByText('Fixed').length).toBeGreaterThan(1));
  },
};

export const RoundComposer: Story = {
  name: 'ラウンドの起票（下書き → 確定）',
  beforeEach: mockFetch(),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const user = userEvent.setup();
    await canvas.findByText('選択状態が URL に反映されず、リロードで詳細ペインが閉じる');

    await user.click(canvas.getByRole('button', { name: '指摘を起票' }));
    const composer = await canvas.findByTestId('round-composer');
    await expect(composer).toHaveTextContent('確定するまでサーバーには作られません');

    // head SHA が未入力のうちは確定できない
    await expect(canvas.getByRole('button', { name: /確定/ })).toBeDisabled();

    // 短縮 SHA も確定できない。API は 40 桁しか受け付けず、通してしまうと
    // そのラウンドは鮮度の照合が永久に一致しなくなる
    await user.type(canvas.getByLabelText('レビュー対象の head SHA'), '9f4e7b2c1a08');
    await expect(canvas.findByTestId('head-sha-error')).resolves.toBeInTheDocument();
    await expect(canvas.getByRole('button', { name: /確定/ })).toBeDisabled();

    await user.clear(canvas.getByLabelText('レビュー対象の head SHA'));
    await user.type(canvas.getByLabelText('レビュー対象の head SHA'), LATEST_HEAD_SHA);
    await user.type(canvas.getByLabelText('タイトル'), '新しい指摘');
    await user.type(canvas.getByLabelText('本文'), '再現条件と根拠');
    await user.click(canvas.getByRole('button', { name: '下書きに追加' }));

    await expect(canvas.findByTestId('staged-list')).resolves.toHaveTextContent('新しい指摘');
    await expect(canvas.getByRole('button', { name: /確定（1 件）/ })).toBeEnabled();
  },
};
