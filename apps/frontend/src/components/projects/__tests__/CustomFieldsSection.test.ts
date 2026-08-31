import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { mount, flushPromises, DOMWrapper, enableAutoUnmount } from '@vue/test-utils';
import { VueQueryPlugin, QueryClient } from '@tanstack/vue-query';

// 一覧は queryOptions を差し替えて本物の useQuery に流す（WorkflowStatusesEditor.test.ts と同じ形）。
// useQuery の戻りごと差し替えると、コンポーネントが options を props に追従させているかを
// 検証できなくなる
const { listQueryFn, createMutateAsync, updateMutateAsync, deleteMutateAsync } = vi.hoisted(() => ({
  listQueryFn: vi.fn(),
  createMutateAsync: vi.fn(),
  updateMutateAsync: vi.fn(),
  deleteMutateAsync: vi.fn(),
}));

vi.mock('@/lib/api-vue-query', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api-vue-query')>();
  return {
    ...actual,
    apiClient: {
      ...actual.apiClient,
      queryOptions: vi.fn((_method: string, path: string, init: unknown) => ({
        queryKey: ['get', path, init],
        queryFn: () => listQueryFn(init),
      })),
      useMutation: vi.fn((method: string) => ({
        mutateAsync:
          method === 'post'
            ? createMutateAsync
            : method === 'patch'
              ? updateMutateAsync
              : deleteMutateAsync,
        isPending: { value: false },
      })),
    },
  };
});

import CustomFieldsSection from '../CustomFieldsSection.vue';
import type { components } from '@/generated/api';

type CustomFieldResponse = components['schemas']['ProjectCustomFieldResponse'];

enableAutoUnmount(afterEach);

const TENANT_UUID = '11111111-1111-1111-1111-111111111111';
const PROJECT_UUID = '00000000-0000-4000-8000-000000000010';

const sampleFields: CustomFieldResponse[] = [
  {
    id: '00000000-0000-4000-8000-000000000101',
    project_id: PROJECT_UUID,
    name: '見積もり',
    field_type: 'number',
    is_required: false,
    position: 0,
    created_at: '2026-01-01T00:00:00Z',
  },
  {
    id: '00000000-0000-4000-8000-000000000102',
    project_id: PROJECT_UUID,
    name: '優先度',
    field_type: 'select',
    options: [{ label: '高', value: '高' }],
    is_required: false,
    position: 1,
    created_at: '2026-01-01T00:00:00Z',
  },
  // backend は label !== value を許可し、タスク表示には label を使う
  // （API 経由で作られたフィールドはこの形になり得る）
  {
    id: '00000000-0000-4000-8000-000000000103',
    project_id: PROJECT_UUID,
    name: '難易度',
    field_type: 'select',
    options: [
      { label: '優先度：高', value: 'high' },
      { label: '低め', value: 'low' },
    ],
    is_required: false,
    position: 2,
    created_at: '2026-01-01T00:00:00Z',
  },
];

function mountSection() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return mount(CustomFieldsSection, {
    props: { tenantId: TENANT_UUID, projectId: PROJECT_UUID },
    global: { plugins: [[VueQueryPlugin, { queryClient }]] },
    attachTo: document.body,
  });
}

function bodyButton(label: string) {
  const button = [...document.body.querySelectorAll('button')].find(
    (b) => (b.textContent?.trim() ?? b.getAttribute('aria-label')) === label,
  );
  if (!button) throw new Error(`button "${label}" not found`);
  return button;
}

function ariaButton(label: string) {
  const button = document.body.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`);
  if (!button) throw new Error(`button[aria-label="${label}"] not found`);
  return button;
}

function nameInput() {
  const el = document.body.querySelector<HTMLInputElement>('#name');
  if (!el) throw new Error('input #name not found');
  return new DOMWrapper(el);
}

function dialogForm() {
  const el = document.body.querySelector('[role="dialog"] form');
  if (!el) throw new Error('dialog form not found');
  return new DOMWrapper(el);
}

describe('CustomFieldsSection', () => {
  beforeEach(() => {
    createMutateAsync.mockReset();
    updateMutateAsync.mockReset();
    deleteMutateAsync.mockReset();
    listQueryFn.mockReset();
    listQueryFn.mockResolvedValue({ fields: sampleFields });
    document.body.innerHTML = '';
  });

  it('一覧: フィールド名と型バッジ（日本語ラベル）を表示する', async () => {
    mountSection();
    await flushPromises();

    const list = document.body.querySelector('ul[aria-label="カスタムフィールド一覧"]');
    expect(list).not.toBeNull();
    expect(list!.textContent).toContain('見積もり');
    expect(list!.textContent).toContain('数値');
    expect(list!.textContent).toContain('優先度');
    expect(list!.textContent).toContain('選択');
  });

  it('空状態: フィールドが無いことを表示する', async () => {
    listQueryFn.mockResolvedValue({ fields: [] });
    mountSection();
    await flushPromises();

    expect(document.body.textContent).toContain('カスタムフィールドはまだありません');
  });

  it('読み込み失敗: エラーを表示する', async () => {
    listQueryFn.mockRejectedValue(new Error('boom'));
    mountSection();
    await flushPromises();

    expect(document.body.textContent).toContain('カスタムフィールドを読み込めませんでした');
  });

  it('追加: 名前と型（既定 text）で POST を送りダイアログを閉じる', async () => {
    createMutateAsync.mockResolvedValue({});
    mountSection();
    await flushPromises();

    bodyButton('フィールドを追加').click();
    await flushPromises();

    await nameInput().setValue('担当チーム');
    await dialogForm().trigger('submit');
    await flushPromises();

    expect(createMutateAsync).toHaveBeenCalledWith({
      params: { path: { tenant_id: TENANT_UUID, project_id: PROJECT_UUID } },
      body: { name: '担当チーム', field_type: 'text' },
    });
    expect(document.body.querySelector('[role="dialog"]')).toBeNull();
  });

  it('追加失敗: エラーを表示しダイアログを開いたままにする', async () => {
    createMutateAsync.mockRejectedValue(new Error('bad request'));
    mountSection();
    await flushPromises();

    bodyButton('フィールドを追加').click();
    await flushPromises();

    await nameInput().setValue('担当チーム');
    await dialogForm().trigger('submit');
    await flushPromises();

    expect(document.body.textContent).toContain('カスタムフィールドを追加できませんでした');
    expect(document.body.querySelector('[role="dialog"]')).not.toBeNull();
  });

  it('名前 100 文字は送信でき、101 文字は送信せずフィールドエラーになる', async () => {
    createMutateAsync.mockResolvedValue({});
    mountSection();
    await flushPromises();

    bodyButton('フィールドを追加').click();
    await flushPromises();

    // 101 文字は送信されない
    await nameInput().setValue('あ'.repeat(101));
    await dialogForm().trigger('submit');
    await flushPromises();
    expect(createMutateAsync).not.toHaveBeenCalled();
    expect(document.body.textContent).toContain('名前は 1〜100 文字で入力してください');

    // 100 文字ちょうどは成功する
    await nameInput().setValue('あ'.repeat(100));
    await dialogForm().trigger('submit');
    await flushPromises();
    expect(createMutateAsync).toHaveBeenCalledTimes(1);
    expect(createMutateAsync.mock.calls[0][0].body.name).toBe('あ'.repeat(100));
  });

  it('絵文字はコードポイント単位で数える（100 個は送信可・101 個は不可、backend の chars() と一致）', async () => {
    createMutateAsync.mockResolvedValue({});
    mountSection();
    await flushPromises();
    bodyButton('フィールドを追加').click();
    await flushPromises();

    // '😀' は UTF-16 で 2 単位・コードポイントで 1。101 個は送信されない
    await nameInput().setValue('😀'.repeat(101));
    await dialogForm().trigger('submit');
    await flushPromises();
    expect(createMutateAsync).not.toHaveBeenCalled();
    expect(document.body.textContent).toContain('名前は 1〜100 文字で入力してください');

    // 絵文字 100 個は送信できる（UTF-16 では 200 単位だが弾かれない）
    await nameInput().setValue('😀'.repeat(100));
    await dialogForm().trigger('submit');
    await flushPromises();
    expect(createMutateAsync).toHaveBeenCalledTimes(1);
    expect(createMutateAsync.mock.calls[0][0].body.name).toBe('😀'.repeat(100));
  });

  it('編集: 名前をプリフィルし、PATCH は name のみ送る（型セレクトは出さない）', async () => {
    updateMutateAsync.mockResolvedValue({});
    mountSection();
    await flushPromises();

    ariaButton('見積もり を編集').click();
    await flushPromises();

    expect((nameInput().element as HTMLInputElement).value).toBe('見積もり');
    expect(document.body.querySelector('#custom-field-type')).toBeNull();

    await nameInput().setValue('ポイント');
    await dialogForm().trigger('submit');
    await flushPromises();

    expect(updateMutateAsync).toHaveBeenCalledWith({
      params: {
        path: {
          tenant_id: TENANT_UUID,
          project_id: PROJECT_UUID,
          field_id: sampleFields[0].id,
        },
      },
      body: { name: 'ポイント' },
    });
    expect(document.body.querySelector('[role="dialog"]')).toBeNull();
  });

  it('編集(select): 既存の選択肢をプリフィルし、options 込みで PATCH する', async () => {
    updateMutateAsync.mockResolvedValue({});
    mountSection();
    await flushPromises();

    ariaButton('優先度 を編集').click();
    await flushPromises();

    // 型セレクトは出ない（編集で型変更不可）
    expect(document.body.querySelector('#custom-field-type')).toBeNull();
    // 既存の選択肢がテキストへ復元される
    const optionsTextarea = document.body.querySelector<HTMLTextAreaElement>('#optionsText');
    expect(optionsTextarea).not.toBeNull();
    expect(optionsTextarea!.value).toBe('高');

    // 選択肢を追加して保存
    await new DOMWrapper(optionsTextarea!).setValue('高\n中');
    await dialogForm().trigger('submit');
    await flushPromises();

    expect(updateMutateAsync).toHaveBeenCalledWith({
      params: {
        path: {
          tenant_id: TENANT_UUID,
          project_id: PROJECT_UUID,
          field_id: sampleFields[1].id,
        },
      },
      body: {
        name: '優先度',
        options: [
          { label: '高', value: '高' },
          { label: '中', value: '中' },
        ],
      },
    });
    expect(document.body.querySelector('[role="dialog"]')).toBeNull();
  });

  it('編集(select): 選択肢を空にすると送信せずエラーを表示する', async () => {
    mountSection();
    await flushPromises();

    ariaButton('優先度 を編集').click();
    await flushPromises();

    const optionsTextarea = document.body.querySelector<HTMLTextAreaElement>('#optionsText');
    await new DOMWrapper(optionsTextarea!).setValue('   ');
    await dialogForm().trigger('submit');
    await flushPromises();

    expect(updateMutateAsync).not.toHaveBeenCalled();
    expect(document.body.textContent).toContain('選択肢を1行に1つ以上入力してください');
  });

  it('編集(select): 名前だけ変えたときは options を送らない（既存の label を壊さない）', async () => {
    updateMutateAsync.mockResolvedValue({});
    mountSection();
    await flushPromises();

    ariaButton('難易度 を編集').click();
    await flushPromises();

    // 編集 UI は value のみを行として表示する
    const optionsTextarea = document.body.querySelector<HTMLTextAreaElement>('#optionsText');
    expect(optionsTextarea!.value).toBe('high\nlow');

    // 選択肢に触れず名前だけ変更する
    await nameInput().setValue('むずかしさ');
    await dialogForm().trigger('submit');
    await flushPromises();

    expect(updateMutateAsync).toHaveBeenCalledWith({
      params: {
        path: {
          tenant_id: TENANT_UUID,
          project_id: PROJECT_UUID,
          field_id: sampleFields[2].id,
        },
      },
      body: { name: 'むずかしさ' },
    });
    // options を送らない＝backend 側の既存 label がそのまま残る
    expect(updateMutateAsync.mock.calls[0][0].body).not.toHaveProperty('options');
  });

  it('編集(select): 選択肢を追加しても既存 option の label は value で上書きされない', async () => {
    updateMutateAsync.mockResolvedValue({});
    mountSection();
    await flushPromises();

    ariaButton('難易度 を編集').click();
    await flushPromises();

    const optionsTextarea = document.body.querySelector<HTMLTextAreaElement>('#optionsText');
    await new DOMWrapper(optionsTextarea!).setValue('high\nlow\nmid');
    await dialogForm().trigger('submit');
    await flushPromises();

    expect(updateMutateAsync.mock.calls[0][0].body.options).toEqual([
      { label: '優先度：高', value: 'high' },
      { label: '低め', value: 'low' },
      { label: 'mid', value: 'mid' },
    ]);
  });

  it('削除: 確認ダイアログを経て DELETE を送る', async () => {
    deleteMutateAsync.mockResolvedValue(undefined);
    mountSection();
    await flushPromises();

    ariaButton('優先度 を削除').click();
    await flushPromises();

    expect(document.body.textContent).toContain('カスタムフィールドを削除しますか？');
    expect(document.body.textContent).toContain('「優先度」を削除します');

    bodyButton('削除する').click();
    await flushPromises();

    expect(deleteMutateAsync).toHaveBeenCalledWith({
      params: {
        path: {
          tenant_id: TENANT_UUID,
          project_id: PROJECT_UUID,
          field_id: sampleFields[1].id,
        },
      },
    });
    expect(document.body.querySelector('[role="dialog"]')).toBeNull();
  });

  it('削除失敗: エラーを表示しダイアログを開いたままにする', async () => {
    deleteMutateAsync.mockRejectedValue(new Error('forbidden'));
    mountSection();
    await flushPromises();

    ariaButton('優先度 を削除').click();
    await flushPromises();
    bodyButton('削除する').click();
    await flushPromises();

    expect(document.body.textContent).toContain('カスタムフィールドを削除できませんでした');
    expect(document.body.querySelector('[role="dialog"]')).not.toBeNull();
  });

  /**
   * projectId が変わったら一覧も取り直す。
   *
   * vike-vue はサイドバーからのプロジェクト切り替えでこのコンポーネントを作り直さない
   * ので、setup 時の props でクエリを組むと一覧だけが前のプロジェクトのまま残る。
   */
  it('projectId が変わったら一覧を取り直す', async () => {
    const OTHER_PROJECT_UUID = '00000000-0000-4000-8000-000000000020';
    listQueryFn.mockImplementation((init: unknown) => {
      const projectId = (init as { params: { path: { project_id: string } } }).params.path
        .project_id;
      return Promise.resolve({
        fields: projectId === PROJECT_UUID ? sampleFields : [],
      });
    });

    const wrapper = mountSection();
    await flushPromises();
    expect(document.body.textContent).toContain('見積もり');

    await wrapper.setProps({ projectId: OTHER_PROJECT_UUID });
    await flushPromises();

    expect(document.body.textContent).toContain('カスタムフィールドはまだありません');
    expect(document.body.textContent).not.toContain('見積もり');
  });
});
