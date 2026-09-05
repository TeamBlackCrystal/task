import type { Meta, StoryObj } from '@storybook/vue3-vite';
import { expect, fn, userEvent, waitFor, within } from 'storybook/test';
import { provide } from 'vue';
import { QueryClient, VUE_QUERY_CLIENT } from '@tanstack/vue-query';

import AccessTokensSection from '@/components/settings/AccessTokensSection.vue';

const USER_ID = '00000000-0000-4000-8000-000000000001';
const OTHER_USER_ID = '00000000-0000-4000-8000-000000000002';
const TENANT_ID = '11111111-1111-1111-1111-111111111111';
const TOKEN_A_ID = '00000000-0000-4000-8000-000000000021';
const TOKEN_B_ID = '00000000-0000-4000-8000-000000000022';

const storyUser = {
  id: USER_ID,
  username: 'shadcn',
  email: 'm@example.com',
  email_verified: true,
  is_admin: false,
  is_suspended: false,
  totp_enabled: false,
  has_password: true,
  bio: null,
  avatar_url: null,
};

const tenant = (ownerId: string) => ({
  id: TENANT_ID,
  display_id: 'acme',
  name: 'Acme Inc',
  description: '',
  icon_url: '',
  owner_id: ownerId,
  drive_quota_bytes: null,
  require_2fa: false,
});

const sampleTokens = [
  {
    id: TOKEN_A_ID,
    name: 'CLI on MacBook',
    token_last_four: '7f3a',
    tenant_id: TENANT_ID,
    project_ids: null,
    scopes: ['read:task', 'write:task', 'read:project'],
    expires_at: '2099-11-14T00:00:00Z',
    last_used_at: '2026-01-01T00:00:00Z',
    revoked: false,
    user_id: USER_ID,
  },
  {
    id: TOKEN_B_ID,
    name: 'CI deploy',
    token_last_four: 'b21c',
    tenant_id: TENANT_ID,
    project_ids: null,
    scopes: ['read:task'],
    expires_at: null,
    last_used_at: null,
    revoked: false,
    user_id: USER_ID,
  },
];

const jsonResponse = (data: unknown, status = 200) =>
  new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });

let fetchSpy: ReturnType<typeof fn> | null = null;

/** トークン系 API をインメモリの配列で応答する fetch モック */
function mockFetch(overrides: { empty?: boolean; tenantOwnerId?: string } = {}) {
  return () => {
    const original = globalThis.fetch;
    let tokens = overrides.empty ? [] : sampleTokens.map((token) => ({ ...token }));
    fetchSpy = fn().mockImplementation(async (req: Request | string) => {
      const url = typeof req === 'string' ? req : req.url;
      const method = typeof req === 'string' ? 'GET' : req.method;
      const pathname = new URL(url, 'http://localhost').pathname;
      const itemMatch = pathname.match(/\/v1\/personal_tokens\/([^/]+)$/);

      if (method === 'GET' && pathname.endsWith('/v1/tenants')) {
        return jsonResponse([tenant(overrides.tenantOwnerId ?? USER_ID)]);
      }
      if (method === 'GET' && pathname.endsWith('/v1/personal_tokens')) {
        return jsonResponse(tokens);
      }
      if (method === 'POST' && pathname.endsWith('/v1/personal_tokens')) {
        const body = await (req as Request).json();
        const created = {
          id: `00000000-0000-4000-8000-0000000000${30 + tokens.length}`,
          name: body.name,
          token_last_four: '9d4e',
          tenant_id: body.tenant_id,
          project_ids: body.project_ids ?? null,
          scopes: body.scopes,
          expires_at: body.expires_at ?? null,
          last_used_at: null,
          revoked: false,
          user_id: USER_ID,
        };
        tokens = [...tokens, created];
        return jsonResponse({ ...created, token: 'pat_plain-token-value-9d4e' }, 201);
      }
      if (method === 'DELETE' && itemMatch) {
        tokens = tokens.filter((token) => token.id !== itemMatch[1]);
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
  title: 'Components/Settings/AccessTokensSection',
  component: AccessTokensSection,
  tags: ['autodocs'],
  args: { user: storyUser },
  parameters: {
    layout: 'padded',
    docs: {
      description: {
        component:
          'アカウント設定のパーソナルアクセストークンセクション。一覧（伏せ字・スコープ数・期限・最終使用）＋発行フォーム＋取り消し（確認ダイアログ）。平文トークンは発行直後にだけ表示する。fetch モックで検証。',
      },
    },
  },
  decorators: [storyDecorator()],
} satisfies Meta<typeof AccessTokensSection>;

export default meta;
type Story = StoryObj<typeof meta>;

const requestsOf = (method: string) =>
  (fetchSpy!.mock.calls as [Request | string][])
    .map(([req]) => req)
    .filter((req): req is Request => typeof req !== 'string')
    .filter((req) => req.method === method);

export const Default: Story = {
  name: '一覧表示（伏せ字＋スコープ数＋期限＋最終使用）',
  beforeEach: mockFetch(),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(
      canvas.findByRole('heading', { name: 'パーソナルアクセストークン' }),
    ).resolves.toBeInTheDocument();
    await expect(canvas.findByText('CLI on MacBook')).resolves.toBeInTheDocument();
    await expect(canvas.getByText('pat_••••••7f3a')).toBeInTheDocument();
    await expect(canvas.getByText('3 スコープ')).toBeInTheDocument();
    await expect(canvas.findByText('CI deploy')).resolves.toBeInTheDocument();
    await expect(canvas.getByText('無期限')).toBeInTheDocument();
    await expect(canvas.getByText('未使用')).toBeInTheDocument();
  },
};

export const Empty: Story = {
  name: '空状態',
  beforeEach: mockFetch({ empty: true }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.findByText('トークンはまだありません。')).resolves.toBeInTheDocument();
  },
};

export const GenerateFlow: Story = {
  name: '発行（フォーム → POST → 平文トークンを 1 度だけ表示）',
  beforeEach: mockFetch({ empty: true }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const user = userEvent.setup();
    await canvas.findByText('トークンはまだありません。');

    await user.click(canvas.getByRole('button', { name: 'トークンを発行' }));
    await user.type(await canvas.findByLabelText('トークン名'), 'CI deploy');
    await user.click(canvas.getByRole('checkbox', { name: /read:task/ }));
    // フォームを開いている間はヘッダー側のボタンが消えるため、この名前は送信ボタンだけ
    await user.click(canvas.getByRole('button', { name: 'トークンを発行' }));

    await expect(canvas.findByTestId('created-token')).resolves.toHaveTextContent(
      'pat_plain-token-value-9d4e',
    );
    await expect(canvas.findByText('pat_••••••9d4e')).resolves.toBeInTheDocument();
    const [post] = requestsOf('POST');
    await expect(post).toBeTruthy();
    await expect(post.url).toContain('/v1/personal_tokens');
  },
};

export const NoOwnedTenant: Story = {
  name: 'オーナーのテナントが無い（発行ボタン無効）',
  beforeEach: mockFetch({ empty: true, tenantOwnerId: OTHER_USER_ID }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(
      canvas.findByText('トークンを発行できるのは、自分がオーナーのテナントだけです。'),
    ).resolves.toBeInTheDocument();
    await expect(canvas.getByRole('button', { name: 'トークンを発行' })).toBeDisabled();
  },
};

export const RevokeFlow: Story = {
  name: '取り消し（確認ダイアログ → DELETE）',
  beforeEach: mockFetch(),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const page = within(canvasElement.ownerDocument.body);
    const user = userEvent.setup();
    await canvas.findByText('CI deploy');

    const revokeButtons = canvas.getAllByRole('button', { name: '取り消し' });
    await user.click(revokeButtons[1]);
    await expect(page.findByText('トークンを取り消しますか？')).resolves.toBeInTheDocument();
    await user.click(page.getByRole('button', { name: '取り消す' }));

    await waitFor(() => expect(canvas.queryByText('CI deploy')).not.toBeInTheDocument());
    const [del] = requestsOf('DELETE');
    await expect(del).toBeTruthy();
    await expect(del.url).toContain(`/v1/personal_tokens/${TOKEN_B_ID}`);
  },
};
