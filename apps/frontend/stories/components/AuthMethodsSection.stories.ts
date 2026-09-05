import type { Meta, StoryObj } from '@storybook/vue3-vite';
import { expect, fn, userEvent, within } from 'storybook/test';
import { provide } from 'vue';
import { QueryClient, VUE_QUERY_CLIENT } from '@tanstack/vue-query';

import AuthMethodsSection from '@/components/settings/AuthMethodsSection.vue';

const USER_ID = '00000000-0000-4000-8000-000000000001';
const INSTANCE_URL = 'https://gitlab.example.com';

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

type Connection = {
  provider: string;
  provider_email: string | null;
  instance_url: string | null;
  connected_at: string;
};

const githubConnection: Connection = {
  provider: 'github',
  provider_email: 'm@example.com',
  instance_url: null,
  connected_at: '2026-04-02T09:30:00Z',
};

const selfHostedConnection: Connection = {
  provider: 'gitlab_selfhosted',
  provider_email: 'dev@example.com',
  instance_url: INSTANCE_URL,
  connected_at: '2026-06-18T12:00:00Z',
};

const allProviders = [
  { provider: 'github', requires_instance_url: false },
  { provider: 'google', requires_instance_url: false },
  { provider: 'gitlab_selfhosted', requires_instance_url: true },
];

const jsonResponse = (data: unknown, status = 200) =>
  new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });

/** 認証方法まわりの API をインメモリで応答する fetch モック */
function mockFetch(
  overrides: {
    connections?: Connection[];
    providers?: typeof allProviders;
    disconnectStatus?: number;
    disconnectMessage?: string;
  } = {},
) {
  return () => {
    const original = globalThis.fetch;
    let connections = (overrides.connections ?? [githubConnection]).map((c) => ({ ...c }));
    const providers = overrides.providers ?? allProviders;

    const fetchSpy = fn().mockImplementation(async (req: Request | string) => {
      const url = typeof req === 'string' ? req : req.url;
      const method = typeof req === 'string' ? 'GET' : req.method;
      const pathname = new URL(url, 'http://localhost').pathname;

      if (pathname.endsWith('/internal/password-strength')) {
        return jsonResponse({ strength: 'high' });
      }
      if (method === 'GET' && pathname.endsWith('/v1/auth/oauth/connections')) {
        return jsonResponse({ connections });
      }
      if (method === 'GET' && pathname.endsWith('/v1/auth/oauth/providers')) {
        return jsonResponse({ providers });
      }
      if (method === 'GET' && pathname.endsWith('/v1/auth/passkeys')) {
        return jsonResponse({ passkeys: [] });
      }
      if (method === 'DELETE' && pathname.includes('/v1/auth/oauth/connections/')) {
        if (overrides.disconnectStatus) {
          return jsonResponse(
            { message: overrides.disconnectMessage ?? 'error' },
            overrides.disconnectStatus,
          );
        }
        const provider = pathname.split('/').pop();
        connections = connections.filter((c) => c.provider !== provider);
        return new Response(null, { status: 204 });
      }
      if (method === 'POST' && pathname.endsWith('/v1/auth/password')) {
        return new Response(null, { status: 204 });
      }
      return jsonResponse({ message: 'not-found' }, 404);
    });

    globalThis.fetch = fetchSpy;
    return () => {
      globalThis.fetch = original;
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
  title: 'Components/Settings/AuthMethodsSection',
  component: AuthMethodsSection,
  tags: ['autodocs'],
  args: { user: storyUser },
  parameters: {
    layout: 'padded',
    docs: {
      description: {
        component:
          'アカウント設定の認証方法セクション。パスワード（初回設定／変更）と OAuth 連携の一覧・追加・解除をまとめる。解除は確認を挟み、最後の認証方法はサーバーが拒む。fetch モックで検証。',
      },
    },
  },
  decorators: [storyDecorator()],
} satisfies Meta<typeof AuthMethodsSection>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  name: 'パスワード設定済み＋連携一覧',
  beforeEach: mockFetch({ connections: [githubConnection, selfHostedConnection] }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.findByRole('heading', { name: '認証方法' })).resolves.toBeInTheDocument();
    await expect(canvas.findByText('設定済み')).resolves.toBeInTheDocument();
    await expect(canvas.findByText('GitHub')).resolves.toBeInTheDocument();
    await expect(canvas.findByText(INSTANCE_URL)).resolves.toBeInTheDocument();
    await expect(canvas.findByText('追加できる連携')).resolves.toBeInTheDocument();
  },
};

export const PasswordNotSet: Story = {
  name: 'パスワード未設定（OAuth のみ・最後の認証方法）',
  args: { user: { ...storyUser, has_password: false } },
  beforeEach: mockFetch({ connections: [githubConnection] }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.findByText('未設定')).resolves.toBeInTheDocument();
    await expect(
      canvas.findByText('OAuth 連携のみでサインインしています'),
    ).resolves.toBeInTheDocument();
    await expect(
      canvas.findByText(/これが最後の認証方法の可能性があります/),
    ).resolves.toBeInTheDocument();
  },
};

export const PasswordChangeForm: Story = {
  name: 'パスワード変更フォーム',
  beforeEach: mockFetch(),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(await canvas.findByRole('button', { name: 'パスワードを変更' }));
    await expect(canvas.findByLabelText('現在のパスワード')).resolves.toBeInTheDocument();
    await expect(
      canvas.findByText(/すべてのセッションとパーソナルアクセストークンが失効します/),
    ).resolves.toBeInTheDocument();
  },
};

export const UnlinkConfirm: Story = {
  name: '解除の確認',
  beforeEach: mockFetch({ connections: [githubConnection, selfHostedConnection] }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const unlinkButtons = await canvas.findAllByRole('button', { name: '解除' });
    await userEvent.click(unlinkButtons[0]);
    await expect(canvas.findByRole('button', { name: '解除する' })).resolves.toBeInTheDocument();
  },
};

export const LastAuthMethodRejected: Story = {
  name: '最後の認証方法は解除できない',
  args: { user: { ...storyUser, has_password: false } },
  beforeEach: mockFetch({
    connections: [githubConnection],
    disconnectStatus: 403,
    disconnectMessage: 'oauth-last-auth-method',
  }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(await canvas.findByRole('button', { name: '解除' }));
    await userEvent.click(await canvas.findByRole('button', { name: '解除する' }));
    await expect(
      canvas.findByText(/これが最後の認証方法のため解除できません/),
    ).resolves.toBeInTheDocument();
  },
};
