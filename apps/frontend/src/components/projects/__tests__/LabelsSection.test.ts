import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { mount, flushPromises, DOMWrapper, enableAutoUnmount } from '@vue/test-utils';
import { VueQueryPlugin, QueryClient, useQuery } from '@tanstack/vue-query';

const { createMutateAsync, updateMutateAsync, deleteMutateAsync, queryState, mutationPending } =
  vi.hoisted(() => ({
    createMutateAsync: vi.fn(),
    updateMutateAsync: vi.fn(),
    deleteMutateAsync: vi.fn(),
    queryState: {
      labels: [] as unknown[],
      isPending: false,
      isError: false,
    },
    // vi.hoisted は import 前に走るため vue の ref は使えない。
    // コンポーネントは .value を読むだけなので素のオブジェクトで足りる
    mutationPending: {
      post: { value: false },
      put: { value: false },
      delete: { value: false },
    },
  }));

// LabelsSection は共有ヘルパー projectLabelsQueryOptions を素の useQuery で消費するため、
// query は @tanstack/vue-query の useQuery をモックする（ヘルパー自体は実物 pass-through）。
vi.mock('@tanstack/vue-query', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@tanstack/vue-query')>();
  return {
    ...actual,
    useQuery: vi.fn(() => ({
      data: { value: queryState.labels },
      isPending: { value: queryState.isPending },
      isError: { value: queryState.isError },
    })),
  };
});

vi.mock('@/lib/api-vue-query', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api-vue-query')>();
  return {
    ...actual,
    apiClient: {
      ...actual.apiClient,
      useMutation: vi.fn((method: string) => ({
        mutateAsync:
          method === 'post'
            ? createMutateAsync
            : method === 'put'
              ? updateMutateAsync
              : deleteMutateAsync,
        isPending:
          method === 'post'
            ? mutationPending.post
            : method === 'put'
              ? mutationPending.put
              : mutationPending.delete,
      })),
    },
  };
});

import LabelsSection from '../LabelsSection.vue';
import type { components } from '@/generated/api';

type LabelResponse = components['schemas']['LabelResponse'];

enableAutoUnmount(afterEach);

const TENANT_UUID = '11111111-1111-1111-1111-111111111111';
const PROJECT_UUID = '00000000-0000-4000-8000-000000000010';

const bugLabel: LabelResponse = {
  id: '00000000-0000-4000-8000-000000000021',
  name: 'bug',
  description: '不具合の報告',
  color: '#ef4444',
  icon_url: null,
  project_id: PROJECT_UUID,
};

const enhancementLabel: LabelResponse = {
  id: '00000000-0000-4000-8000-000000000022',
  name: 'enhancement',
  description: '機能改善の提案',
  color: '#3b82f6',
  icon_url: null,
  project_id: PROJECT_UUID,
};

function mountView() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return mount(LabelsSection, {
    props: { tenantId: TENANT_UUID, projectId: PROJECT_UUID },
    global: { plugins: [[VueQueryPlugin, { queryClient }]] },
    attachTo: document.body,
  });
}

function input(id: string) {
  const el = document.body.querySelector<HTMLInputElement | HTMLTextAreaElement>(`#${id}`);
  if (!el) throw new Error(`input #${id} not found`);
  return new DOMWrapper(el);
}

function formEl() {
  const el = document.body.querySelector('form');
  if (!el) throw new Error('form not found');
  return new DOMWrapper(el);
}

function clickBodyButton(label: string) {
  const button = [...document.body.querySelectorAll('button')].find(
    (b) => b.textContent?.trim() === label,
  );
  if (!button) throw new Error(`button "${label}" not found`);
  button.click();
}

function clickByAriaLabel(label: string) {
  const button = document.body.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`);
  if (!button) throw new Error(`button[aria-label="${label}"] not found`);
  button.click();
}

describe('LabelsSection', () => {
  beforeEach(() => {
    createMutateAsync.mockReset();
    updateMutateAsync.mockReset();
    deleteMutateAsync.mockReset();
    queryState.labels = [];
    queryState.isPending = false;
    queryState.isError = false;
    mutationPending.post.value = false;
    mutationPending.put.value = false;
    mutationPending.delete.value = false;
    document.body.innerHTML = '';
  });

  it('ラベルを行形式で一覧表示する', async () => {
    queryState.labels = [bugLabel, enhancementLabel];
    mountView();
    await flushPromises();

    expect(document.body.textContent).toContain('bug');
    expect(document.body.textContent).toContain('不具合の報告');
    expect(document.body.textContent).toContain('enhancement');
    expect(document.body.textContent).toContain('機能改善の提案');
    expect(document.body.querySelector('button[aria-label="ラベル「bug」を編集"]')).not.toBeNull();
    expect(document.body.querySelector('button[aria-label="ラベル「bug」を削除"]')).not.toBeNull();
  });

  it('ラベルが 0 件なら空状態を表示する', async () => {
    mountView();
    await flushPromises();

    expect(document.body.textContent).toContain('ラベルはまだありません');
  });

  it('作成ダイアログから POST を送り、成功でダイアログを閉じる', async () => {
    createMutateAsync.mockResolvedValue({ ...bugLabel, name: 'design' });
    mountView();
    await flushPromises();

    clickBodyButton('新しいラベル');
    await flushPromises();
    expect(document.body.textContent).toContain('ラベルを作成');

    await input('label-name').setValue('design');
    await input('label-description').setValue('デザイン関連');
    await formEl().trigger('submit');
    await flushPromises();

    expect(createMutateAsync).toHaveBeenCalledWith({
      params: { path: { tenant_id: TENANT_UUID, project_id: PROJECT_UUID } },
      body: { name: 'design', color: '#ef4444', description: 'デザイン関連' },
    });
    expect(document.body.textContent).not.toContain('ラベルを作成');
  });

  it('編集ダイアログは既存値をプリフィルし、保存で PUT を送る', async () => {
    updateMutateAsync.mockResolvedValue({ ...bugLabel, name: 'defect' });
    queryState.labels = [bugLabel];
    mountView();
    await flushPromises();

    clickByAriaLabel('ラベル「bug」を編集');
    await flushPromises();

    expect((input('label-name').element as HTMLInputElement).value).toBe('bug');
    expect((input('label-color').element as HTMLInputElement).value).toBe('#ef4444');

    await input('label-name').setValue('defect');
    await formEl().trigger('submit');
    await flushPromises();

    expect(updateMutateAsync).toHaveBeenCalledWith({
      params: {
        path: { tenant_id: TENANT_UUID, project_id: PROJECT_UUID, id: bugLabel.id },
      },
      body: { name: 'defect', color: '#ef4444', description: '不具合の報告' },
    });
  });

  it('名前が空・色が不正なら送信せずエラーを表示する', async () => {
    mountView();
    await flushPromises();

    clickBodyButton('新しいラベル');
    await flushPromises();

    await input('label-color').setValue('red');
    await formEl().trigger('submit');
    await flushPromises();

    expect(createMutateAsync).not.toHaveBeenCalled();
    expect(document.body.textContent).toContain('名前は 1〜100 文字で入力してください');
    expect(document.body.textContent).toContain('色は #RRGGBB 形式で入力してください');
  });

  it('名前 100 文字は送信でき、101 文字は送信せずエラーを表示する', async () => {
    createMutateAsync.mockResolvedValue({ ...bugLabel, name: 'a'.repeat(100) });
    mountView();
    await flushPromises();

    clickBodyButton('新しいラベル');
    await flushPromises();

    await input('label-name').setValue('a'.repeat(101));
    await formEl().trigger('submit');
    await flushPromises();
    expect(createMutateAsync).not.toHaveBeenCalled();
    expect(document.body.textContent).toContain('名前は 1〜100 文字で入力してください');

    // 100 文字ちょうどは成功する
    await input('label-name').setValue('a'.repeat(100));
    await formEl().trigger('submit');
    await flushPromises();
    expect(createMutateAsync).toHaveBeenCalledTimes(1);
    expect(createMutateAsync.mock.calls[0][0].body.name).toBe('a'.repeat(100));
  });

  it('絵文字はコードポイント単位で数える（100 個は送信可・101 個は不可、backend の chars() と一致）', async () => {
    // '😀' は UTF-16 で 2 単位・コードポイントで 1。String.length なら 100 個で 200 と数え
    // 誤って弾くが、Array.from では 100 と数え backend（chars().count()）と一致する
    createMutateAsync.mockResolvedValue({ ...bugLabel, name: '😀'.repeat(100) });
    mountView();
    await flushPromises();

    clickBodyButton('新しいラベル');
    await flushPromises();

    // 絵文字 101 個は送信されない
    await input('label-name').setValue('😀'.repeat(101));
    await formEl().trigger('submit');
    await flushPromises();
    expect(createMutateAsync).not.toHaveBeenCalled();
    expect(document.body.textContent).toContain('名前は 1〜100 文字で入力してください');

    // 絵文字 100 個は送信できる（UTF-16 では 200 単位だが弾かれない）
    await input('label-name').setValue('😀'.repeat(100));
    await formEl().trigger('submit');
    await flushPromises();
    expect(createMutateAsync).toHaveBeenCalledTimes(1);
    expect(createMutateAsync.mock.calls[0][0].body.name).toBe('😀'.repeat(100));
  });

  it('保存リクエスト進行中はフォームダイアログのクローズ要求を無視する', async () => {
    mutationPending.post.value = true;
    const wrapper = mountView();
    await flushPromises();

    clickBodyButton('新しいラベル');
    await flushPromises();
    expect(document.body.textContent).toContain('ラベルを作成');

    // reka-ui の update:open(false)（Esc 等）を直接シミュレート
    const dialogRoot = wrapper.findComponent({ name: 'DialogRoot' });
    dialogRoot.vm.$emit('update:open', false);
    await flushPromises();

    expect(document.body.textContent).toContain('ラベルを作成');
  });

  it('削除リクエスト進行中は確認ダイアログのクローズ要求を無視する', async () => {
    mutationPending.delete.value = true;
    queryState.labels = [bugLabel];
    const wrapper = mountView();
    await flushPromises();

    clickByAriaLabel('ラベル「bug」を削除');
    await flushPromises();
    expect(document.body.textContent).toContain('ラベルを削除しますか？');

    const dialogRoot = wrapper.findComponent({ name: 'DialogRoot' });
    dialogRoot.vm.$emit('update:open', false);
    await flushPromises();

    expect(document.body.textContent).toContain('ラベルを削除しますか？');
  });

  it('行の削除ボタン → 確認ダイアログで DELETE を送る', async () => {
    deleteMutateAsync.mockResolvedValue(undefined);
    queryState.labels = [bugLabel];
    mountView();
    await flushPromises();

    clickByAriaLabel('ラベル「bug」を削除');
    await flushPromises();

    expect(document.body.textContent).toContain('「bug」を削除します。この操作は取り消せません。');

    clickBodyButton('削除する');
    await flushPromises();

    expect(deleteMutateAsync).toHaveBeenCalledWith({
      params: {
        path: { tenant_id: TENANT_UUID, project_id: PROJECT_UUID, id: bugLabel.id },
      },
    });
    expect(document.body.textContent).not.toContain('ラベルを削除しますか？');
  });

  it('保存失敗時はダイアログ内にエラーを表示する', async () => {
    createMutateAsync.mockRejectedValue(new Error('forbidden'));
    mountView();
    await flushPromises();

    clickBodyButton('新しいラベル');
    await flushPromises();
    await input('label-name').setValue('design');
    await formEl().trigger('submit');
    await flushPromises();

    expect(document.body.textContent).toContain('ラベルを保存できませんでした');
  });

  it('削除失敗時は確認ダイアログ内にエラーを表示する', async () => {
    deleteMutateAsync.mockRejectedValue(new Error('forbidden'));
    queryState.labels = [bugLabel];
    mountView();
    await flushPromises();

    clickByAriaLabel('ラベル「bug」を削除');
    await flushPromises();
    clickBodyButton('削除する');
    await flushPromises();

    expect(document.body.textContent).toContain('ラベルを削除できませんでした');
    expect(document.body.textContent).toContain('ラベルを削除しますか？');
  });

  /**
   * projectId が変わったらクエリキーも切り替わる。
   *
   * vike-vue はサイドバーからのプロジェクト切り替えでこのコンポーネントを作り直さない
   * ので、setup 時の props で options を組むと一覧だけが前のプロジェクトのまま残る。
   * ここでは useQuery ごとモックしているため、渡している options が props に追従して
   * 再評価されることを見る。
   */
  it('projectId が変わったらクエリキーが切り替わる', async () => {
    const OTHER_PROJECT_UUID = '00000000-0000-4000-8000-000000000020';
    queryState.labels = [bugLabel];
    const wrapper = mountView();
    await flushPromises();

    const optionsRef = vi.mocked(useQuery).mock.calls.at(-1)?.[0] as {
      value: { queryKey: readonly unknown[] };
    };
    expect(JSON.stringify(optionsRef.value.queryKey)).toContain(PROJECT_UUID);

    await wrapper.setProps({ projectId: OTHER_PROJECT_UUID });
    await flushPromises();

    expect(JSON.stringify(optionsRef.value.queryKey)).toContain(OTHER_PROJECT_UUID);
  });
});
