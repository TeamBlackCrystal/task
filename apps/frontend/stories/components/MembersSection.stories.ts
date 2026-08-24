import type { Meta, StoryObj } from '@storybook/vue3-vite';
import { expect, fn, userEvent, waitFor, within } from 'storybook/test';
import { provide } from 'vue';
import { QueryClient, VUE_QUERY_CLIENT } from '@tanstack/vue-query';

import MembersSection from '@/components/projects/MembersSection.vue';

const TENANT_UUID = '11111111-1111-1111-1111-111111111111';
const PROJECT_UUID = '00000000-0000-4000-8000-000000000010';
const ALICE_ID = '00000000-0000-4000-8000-0000000000aa';
const BOB_ID = '00000000-0000-4000-8000-0000000000bb';
const CAROL_ID = '00000000-0000-4000-8000-0000000000cc';

const user = (id: string, username: string) => ({ id, username, avatar_url: null });

const sampleMembers = [
  {
    id: '00000000-0000-4000-8000-000000000031',
    project_id: PROJECT_UUID,
    user_id: ALICE_ID,
    role: 'Admin',
    user: user(ALICE_ID, 'alice'),
  },
  {
    id: '00000000-0000-4000-8000-000000000032',
    project_id: PROJECT_UUID,
    user_id: BOB_ID,
    role: 'Member',
    user: user(BOB_ID, 'bob'),
  },
];

const sampleTenantMembers = [
  {
    id: '00000000-0000-4000-8000-000000000041',
    tenant_id: TENANT_UUID,
    user_id: ALICE_ID,
    role: 'Member',
    user: user(ALICE_ID, 'alice'),
  },
  {
    id: '00000000-0000-4000-8000-000000000042',
    tenant_id: TENANT_UUID,
    user_id: BOB_ID,
    role: 'Member',
    user: user(BOB_ID, 'bob'),
  },
  {
    id: '00000000-0000-4000-8000-000000000043',
    tenant_id: TENANT_UUID,
    user_id: CAROL_ID,
    role: 'Member',
    user: user(CAROL_ID, 'carol'),
  },
];

const jsonResponse = (data: unknown, status = 200) =>
  new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });

let fetchSpy: ReturnType<typeof fn> | null = null;

/** メンバー系 API をインメモリの配列で応答する fetch モック */
function mockFetch(
  overrides: { empty?: boolean; listStatus?: number; mutationStatus?: number } = {},
) {
  return () => {
    const original = globalThis.fetch;
    let members = overrides.empty ? [] : sampleMembers.map((member) => ({ ...member }));
    fetchSpy = fn().mockImplementation(async (req: Request | string) => {
      const url = typeof req === 'string' ? req : req.url;
      const method = typeof req === 'string' ? 'GET' : req.method;
      const pathname = new URL(url, 'http://localhost').pathname;
      const itemMatch = pathname.match(/\/projects\/[^/]+\/members\/([^/]+)$/);

      if (method === 'GET' && pathname.endsWith(`/v1/tenants/${TENANT_UUID}/members`)) {
        return jsonResponse(sampleTenantMembers);
      }
      if (method === 'GET' && pathname.endsWith(`/projects/${PROJECT_UUID}/members`)) {
        if (overrides.listStatus) return jsonResponse({ message: 'error' }, overrides.listStatus);
        return jsonResponse(members);
      }
      if (method === 'POST' && pathname.endsWith(`/projects/${PROJECT_UUID}/members`)) {
        const body = await (req as Request).json();
        if (overrides.mutationStatus)
          return jsonResponse({ message: 'error' }, overrides.mutationStatus);
        const tenantMember = sampleTenantMembers.find((tm) => tm.user_id === body.user_id);
        const created = {
          id: `00000000-0000-4000-8000-0000000000${50 + members.length}`,
          project_id: PROJECT_UUID,
          user_id: body.user_id,
          role: body.role,
          user: tenantMember?.user ?? user(body.user_id, 'unknown'),
        };
        members = [...members, created];
        return jsonResponse(created, 201);
      }
      if (method === 'PUT' && itemMatch) {
        const body = await (req as Request).json();
        if (overrides.mutationStatus)
          return jsonResponse({ message: 'error' }, overrides.mutationStatus);
        members = members.map((member) =>
          member.user_id === itemMatch[1] ? { ...member, role: body.role } : member,
        );
        return jsonResponse(members.find((member) => member.user_id === itemMatch[1]));
      }
      if (method === 'DELETE' && itemMatch) {
        if (overrides.mutationStatus)
          return jsonResponse({ message: 'error' }, overrides.mutationStatus);
        members = members.filter((member) => member.user_id !== itemMatch[1]);
        return new Response(null, { status: 204 });
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
  title: 'Components/Projects/MembersSection',
  component: MembersSection,
  tags: ['autodocs'],
  args: { tenantId: TENANT_UUID, projectId: PROJECT_UUID },
  parameters: {
    layout: 'padded',
    docs: {
      description: {
        component:
          'プロジェクト設定のメンバーセクション。一覧（アバター・ユーザー名・ロール）＋テナントメンバーからの追加＋ロール変更＋削除（確認ダイアログ）。fetch モックで検証。',
      },
    },
  },
  decorators: [storyDecorator()],
} satisfies Meta<typeof MembersSection>;

export default meta;
type Story = StoryObj<typeof meta>;

const requestsOf = (method: string) =>
  (fetchSpy!.mock.calls as [Request | string][])
    .map(([req]) => req)
    .filter((req): req is Request => typeof req !== 'string')
    .filter((req) => req.method === method);

export const Default: Story = {
  name: '一覧表示（アバター＋ユーザー名＋ロール＋削除ボタン）',
  beforeEach: mockFetch(),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.findByRole('heading', { name: 'メンバー' })).resolves.toBeInTheDocument();
    await expect(canvas.findByText('alice')).resolves.toBeInTheDocument();
    await expect(canvas.findByText('bob')).resolves.toBeInTheDocument();
    await expect(canvas.getByText('管理者')).toBeInTheDocument();
    await expect(
      canvas.getByRole('button', { name: 'メンバー「alice」を削除' }),
    ).toBeInTheDocument();
  },
};

export const Empty: Story = {
  name: '空状態（メンバー未指定＝テナント全体に開放）',
  beforeEach: mockFetch({ empty: true }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(
      canvas.findByText('メンバーはまだ指定されていません。'),
    ).resolves.toBeInTheDocument();
  },
};

export const Forbidden: Story = {
  name: '403（管理者以外は閲覧・操作不可）',
  beforeEach: mockFetch({ listStatus: 403 }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(
      canvas.findByText(
        'メンバーを管理できるのは、テナントオーナーとこのプロジェクトの管理者だけです。',
      ),
    ).resolves.toBeInTheDocument();
    await expect(canvas.queryByText('テナントメンバーから追加')).not.toBeInTheDocument();
  },
};

export const AddFlow: Story = {
  name: '追加（候補を選択 → POST → 一覧反映）',
  beforeEach: mockFetch(),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const page = within(canvasElement.ownerDocument.body);
    const user = userEvent.setup();
    await canvas.findByText('alice');

    // 候補は未参加のテナントメンバー（carol）だけ
    await user.click(canvas.getByLabelText('テナントメンバーから追加'));
    const option = await page.findByRole('option', { name: 'carol' });
    await user.click(option);
    await user.click(canvas.getByRole('button', { name: '追加' }));

    await expect(canvas.findByText('carol')).resolves.toBeInTheDocument();
    const [post] = requestsOf('POST');
    await expect(post).toBeTruthy();
    await expect(post.url).toContain(`/projects/${PROJECT_UUID}/members`);
  },
};

export const RemoveFlow: Story = {
  name: '削除（確認ダイアログ → DELETE）',
  beforeEach: mockFetch(),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const page = within(canvasElement.ownerDocument.body);
    const user = userEvent.setup();
    await canvas.findByText('bob');
    await user.click(canvas.getByRole('button', { name: 'メンバー「bob」を削除' }));
    await expect(page.findByText('メンバーを削除しますか？')).resolves.toBeInTheDocument();
    await user.click(page.getByRole('button', { name: '削除する' }));

    await waitFor(() => expect(canvas.queryByText('bob')).not.toBeInTheDocument());
    const [del] = requestsOf('DELETE');
    await expect(del).toBeTruthy();
    await expect(del.url).toContain(`/members/${BOB_ID}`);
  },
};

export const LastAdminGuard: Story = {
  name: 'ロール変更の 409（最後の管理者は降格できない）',
  beforeEach: mockFetch({ mutationStatus: 409 }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const page = within(canvasElement.ownerDocument.body);
    const user = userEvent.setup();
    await canvas.findByText('alice');

    await user.click(canvas.getByLabelText('alice のロール'));
    await user.click(await page.findByRole('option', { name: /メンバー/ }));

    await expect(
      canvas.findByText('最後の管理者「alice」は降格できません。'),
    ).resolves.toBeInTheDocument();
    const [put] = requestsOf('PUT');
    await expect(put).toBeTruthy();
  },
};
