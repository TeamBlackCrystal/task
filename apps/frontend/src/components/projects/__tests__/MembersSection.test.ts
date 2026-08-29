import { describe, it, expect, afterEach, vi } from 'vitest';
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils';
import { VueQueryPlugin, QueryClient } from '@tanstack/vue-query';
import MembersSection from '../MembersSection.vue';
import { Select } from '@/components/ui/select';
import type { components } from '@/generated/api';

const TENANT_ID = '11111111-1111-1111-1111-111111111111';
const PROJECT_ID = '00000000-0000-4000-8000-000000000010';
const ALICE_ID = '00000000-0000-0000-0000-00000000000a';
const BOB_ID = '00000000-0000-0000-0000-00000000000b';

function user(id: string, username: string): components['schemas']['UserSummary'] {
  return { id, username, avatar_url: null };
}

function projectMember(
  userId: string,
  username: string,
  role: components['schemas']['ProjectRole'],
): components['schemas']['ProjectMemberResponse'] {
  return {
    id: `pm-${userId}`,
    project_id: PROJECT_ID,
    user_id: userId,
    role,
    user: user(userId, username),
  };
}

function tenantMember(
  userId: string,
  username: string,
): components['schemas']['TenantMemberResponse'] {
  return {
    id: `tm-${userId}`,
    tenant_id: TENANT_ID,
    user_id: userId,
    role: 'Member',
    user: user(userId, username),
  };
}

type MockState = {
  members: components['schemas']['ProjectMemberResponse'][];
  tenantMembers: components['schemas']['TenantMemberResponse'][];
  /** 400 以上を設定すると GET members が失敗する */
  listStatus?: number;
  /** 400 以上を設定すると GET tenant members（追加候補）が失敗する */
  tenantListStatus?: number;
  addStatus?: number;
  updateStatus?: number;
  deleteStatus?: number;
};

const jsonResponse = (data: unknown, status = 200) =>
  new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });

function stubFetch(state: MockState) {
  const addBodies: unknown[] = [];
  const putRequests: { path: string; body: unknown }[] = [];
  const deletedPaths: string[] = [];
  const fetchMock = vi.fn(async (req: Request) => {
    const pathname = new URL(req.url, 'http://localhost').pathname;
    const memberItemMatch = pathname.match(/\/projects\/[^/]+\/members\/([^/]+)$/);

    if (req.method === 'GET' && pathname.endsWith(`/v1/tenants/${TENANT_ID}/members`)) {
      if (state.tenantListStatus) {
        return jsonResponse({ message: 'error' }, state.tenantListStatus);
      }
      return jsonResponse(state.tenantMembers);
    }
    if (req.method === 'GET' && pathname.endsWith(`/projects/${PROJECT_ID}/members`)) {
      if (state.listStatus) return jsonResponse({ message: 'error' }, state.listStatus);
      return jsonResponse(state.members);
    }
    if (req.method === 'POST' && pathname.endsWith(`/projects/${PROJECT_ID}/members`)) {
      const body = (await req.clone().json()) as { user_id: string; role: string };
      addBodies.push(body);
      if (state.addStatus) return jsonResponse({ message: 'error' }, state.addStatus);
      const tm = state.tenantMembers.find((m) => m.user_id === body.user_id);
      const created = projectMember(
        body.user_id,
        tm?.user.username ?? 'unknown',
        body.role as components['schemas']['ProjectRole'],
      );
      state.members = [...state.members, created];
      return jsonResponse(created, 201);
    }
    if (req.method === 'PUT' && memberItemMatch) {
      putRequests.push({ path: pathname, body: await req.clone().json() });
      if (state.updateStatus) return jsonResponse({ message: 'error' }, state.updateStatus);
      const userId = memberItemMatch[1];
      state.members = state.members.map((m) =>
        m.user_id === userId
          ? { ...m, role: (putRequests.at(-1)!.body as { role: never }).role }
          : m,
      );
      return jsonResponse(state.members.find((m) => m.user_id === userId));
    }
    if (req.method === 'DELETE' && memberItemMatch) {
      deletedPaths.push(pathname);
      if (state.deleteStatus) return jsonResponse({ message: 'error' }, state.deleteStatus);
      state.members = state.members.filter((m) => m.user_id !== memberItemMatch[1]);
      return new Response(null, { status: 204 });
    }
    return jsonResponse({ message: 'not-found' }, 404);
  });
  vi.stubGlobal('fetch', fetchMock);
  return { addBodies, putRequests, deletedPaths };
}

function mountSection() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return mount(MembersSection, {
    props: { tenantId: TENANT_ID, projectId: PROJECT_ID },
    global: { plugins: [[VueQueryPlugin, { queryClient }]] },
    attachTo: document.body,
  });
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
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('MembersSection', () => {
  it('メンバーの名前とロールを一覧に表示する', async () => {
    stubFetch({
      members: [projectMember(ALICE_ID, 'alice', 'Admin'), projectMember(BOB_ID, 'bob', 'Member')],
      tenantMembers: [tenantMember(ALICE_ID, 'alice'), tenantMember(BOB_ID, 'bob')],
    });
    const wrapper = mountSection();
    await flushPromises();

    const list = wrapper.get('[data-testid="member-list"]');
    expect(list.text()).toContain('alice');
    expect(list.text()).toContain('bob');
    expect(list.text()).toContain('管理者');
    expect(list.text()).toContain('メンバー');
  });

  it('403 のときは管理者専用であることを表示し、追加フォームを出さない', async () => {
    stubFetch({ members: [], tenantMembers: [], listStatus: 403 });
    const wrapper = mountSection();
    await flushPromises();

    expect(wrapper.text()).toContain(
      'メンバーを管理できるのは、テナントオーナーとこのプロジェクトの管理者だけです。',
    );
    expect(wrapper.find('#member-candidate').exists()).toBe(false);
  });

  it('候補を選んで追加すると POST を送り、一覧に反映される', async () => {
    const { addBodies } = stubFetch({
      members: [projectMember(ALICE_ID, 'alice', 'Admin')],
      tenantMembers: [tenantMember(ALICE_ID, 'alice'), tenantMember(BOB_ID, 'bob')],
    });
    const wrapper = mountSection();
    await flushPromises();

    // reka-ui の Select はポインタ操作を jsdom で再現しづらいため、モデル更新で選択する
    const candidateSelect = wrapper
      .findAllComponents(Select)
      .find((s) => s.find('#member-candidate').exists());
    if (!candidateSelect) throw new Error('candidate select not found');
    candidateSelect.vm.$emit('update:modelValue', BOB_ID);
    await flushPromises();

    clickBodyButton('追加');
    await flushPromises();

    expect(addBodies).toEqual([{ user_id: BOB_ID, role: 'Member' }]);
    expect(wrapper.get('[data-testid="member-list"]').text()).toContain('bob');
  });

  it('全テナントメンバーが参加済みなら追加候補が無いことを表示する', async () => {
    stubFetch({
      members: [projectMember(ALICE_ID, 'alice', 'Admin')],
      tenantMembers: [tenantMember(ALICE_ID, 'alice')],
    });
    const wrapper = mountSection();
    await flushPromises();

    expect(wrapper.text()).toContain('追加できるテナントメンバーがいません。');
    expect(bodyButton('追加')?.disabled).toBe(true);
  });

  it('追加候補の取得に失敗したら「いません」ではなく失敗として伝える', async () => {
    stubFetch({
      members: [projectMember(ALICE_ID, 'alice', 'Admin')],
      // テナントには人が居るが、取得に失敗する
      tenantMembers: [tenantMember(BOB_ID, 'bob')],
      tenantListStatus: 500,
    });
    const wrapper = mountSection();
    await flushPromises();

    expect(wrapper.text()).toContain('追加候補を読み込めませんでした');
    // 「テナントに人が居ない」という別の事実にすり替えない
    expect(wrapper.text()).not.toContain('追加できるテナントメンバーがいません。');
    // メンバー一覧は読めているので出したまま
    expect(wrapper.get('[data-testid="member-list"]').text()).toContain('alice');
  });

  it('ロール変更の 409 は最後の管理者であることを伝える', async () => {
    const { putRequests } = stubFetch({
      members: [projectMember(ALICE_ID, 'alice', 'Admin')],
      tenantMembers: [tenantMember(ALICE_ID, 'alice')],
      updateStatus: 409,
    });
    const wrapper = mountSection();
    await flushPromises();

    const roleSelect = wrapper
      .findAllComponents(Select)
      .find((s) => s.find('[aria-label="alice のロール"]').exists());
    if (!roleSelect) throw new Error('role select not found');
    roleSelect.vm.$emit('update:modelValue', 'Member');
    await flushPromises();

    expect(putRequests).toHaveLength(1);
    expect(putRequests[0].body).toEqual({ role: 'Member' });
    expect(wrapper.text()).toContain('最後の管理者「alice」は降格できません。');
  });

  it('ロール変更に成功すると PUT を送り、表示が新しいロールになる', async () => {
    const { putRequests } = stubFetch({
      members: [projectMember(ALICE_ID, 'alice', 'Admin'), projectMember(BOB_ID, 'bob', 'Viewer')],
      tenantMembers: [],
    });
    const wrapper = mountSection();
    await flushPromises();

    const roleSelect = wrapper
      .findAllComponents(Select)
      .find((s) => s.find('[aria-label="bob のロール"]').exists());
    if (!roleSelect) throw new Error('role select not found');
    roleSelect.vm.$emit('update:modelValue', 'Member');
    await flushPromises();

    expect(putRequests).toHaveLength(1);
    expect(putRequests[0].path.endsWith(`/members/${BOB_ID}`)).toBe(true);
    expect(wrapper.get('[data-testid="member-list"]').text()).toContain('メンバー');
  });

  it('削除は確認ダイアログを経て DELETE を送り、一覧から消す', async () => {
    const { deletedPaths } = stubFetch({
      members: [projectMember(ALICE_ID, 'alice', 'Admin'), projectMember(BOB_ID, 'bob', 'Member')],
      tenantMembers: [],
    });
    const wrapper = mountSection();
    await flushPromises();

    const removeButton = document.body.querySelector<HTMLButtonElement>(
      'button[aria-label="メンバー「bob」を削除"]',
    );
    if (!removeButton) throw new Error('remove button not found');
    removeButton.click();
    await flushPromises();

    expect(document.body.textContent).toContain('メンバーを削除しますか？');

    clickBodyButton('削除する');
    await flushPromises();

    expect(deletedPaths).toHaveLength(1);
    expect(deletedPaths[0].endsWith(`/members/${BOB_ID}`)).toBe(true);
    expect(wrapper.get('[data-testid="member-list"]').text()).not.toContain('bob');
  });

  it('確認ダイアログでキャンセルすると DELETE を送らない', async () => {
    const { deletedPaths } = stubFetch({
      members: [projectMember(ALICE_ID, 'alice', 'Admin')],
      tenantMembers: [],
    });
    const wrapper = mountSection();
    await flushPromises();

    document.body
      .querySelector<HTMLButtonElement>('button[aria-label="メンバー「alice」を削除"]')
      ?.click();
    await flushPromises();
    clickBodyButton('キャンセル');
    await flushPromises();

    expect(deletedPaths).toHaveLength(0);
    expect(wrapper.get('[data-testid="member-list"]').text()).toContain('alice');
  });

  it('削除の 409 は最後の管理者であることを伝え、一覧は残る', async () => {
    stubFetch({
      members: [projectMember(ALICE_ID, 'alice', 'Admin')],
      tenantMembers: [],
      deleteStatus: 409,
    });
    const wrapper = mountSection();
    await flushPromises();

    document.body
      .querySelector<HTMLButtonElement>('button[aria-label="メンバー「alice」を削除"]')
      ?.click();
    await flushPromises();
    clickBodyButton('削除する');
    await flushPromises();

    expect(document.body.textContent).toContain('最後の管理者は削除できません。');
    expect(wrapper.get('[data-testid="member-list"]').text()).toContain('alice');
  });
});
