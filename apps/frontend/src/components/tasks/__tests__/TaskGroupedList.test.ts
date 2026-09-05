import { afterEach, describe, expect, it, vi } from 'vitest';
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils';
import { nextTick } from 'vue';

import TaskGroupedList from '@/components/tasks/TaskGroupedList.vue';
import type { TaskGroup } from '@/components/tasks/task-grouped-columns';
import type { components } from '@/generated/api';
import type { CreateTaskInput } from '@/composables/useTaskRowMutations';

enableAutoUnmount(afterEach);

type StatusResponse = components['schemas']['ProjectStatusResponse'];
type LabelResponse = components['schemas']['LabelResponse'];

const status: StatusResponse = {
  id: 'status-todo',
  name: 'Todo',
  color: '#94a3b8',
  position: 0,
  is_default: true,
  is_done_state: false,
  project_id: 'project-1',
  created_at: '2026-06-01T00:00:00Z',
};

const bug: LabelResponse = {
  id: 'label-bug',
  name: 'bug',
  description: '',
  color: '#e11d48',
  icon_url: null,
  project_id: 'project-1',
};

const members = [
  { id: 'user-1', username: 'yupix', avatar_url: null },
  { id: 'user-2', username: 'sousuke', avatar_url: null },
];

const group: TaskGroup = {
  status,
  tasks: [],
  total: 0,
  isLoading: false,
  isError: false,
  hasMore: false,
  retry: () => {},
  loadMore: () => {},
};

const doingStatus: StatusResponse = { ...status, id: 'status-doing', name: 'Doing', position: 1 };
const doingGroup: TaskGroup = { ...group, status: doingStatus };

/** 作成の受け口。成否を返す契約なので、既定は成功にする。 */
function mountList(
  onCreate = vi.fn(async (_input: CreateTaskInput) => true),
  groups: TaskGroup[] = [group],
  statuses: StatusResponse[] = [status],
) {
  const wrapper = mount(TaskGroupedList, {
    props: {
      groups,
      statuses,
      projectLabels: [bug],
      members,
      pending: {},
      errors: {},
      onComment: vi.fn(async () => true),
      onCreate,
    },
    attachTo: document.body,
  });
  return { wrapper, onCreate };
}

/** グループは `section` 単位。同じ文言のボタンが並ぶので、対象の節の中から探す。 */
function sectionOf(wrapper: ReturnType<typeof mountList>['wrapper'], statusName: string) {
  const section = wrapper
    .findAll('section')
    .find((s) => s.find(`[aria-label="${statusName} を折りたたむ"]`).exists());
  expect(section).toBeDefined();
  return section!;
}

async function openAddRow(wrapper: ReturnType<typeof mountList>['wrapper'], statusName = 'Todo') {
  const addButton = sectionOf(wrapper, statusName)
    .findAll('button')
    .find((b) => b.text() === 'タスクを追加');
  expect(addButton).toBeDefined();
  await addButton!.trigger('click');
  await nextTick();
}

describe('TaskGroupedList のタスク追加', () => {
  it('タイトルだけでも作れる（未指定の項目は送らない）', async () => {
    const { wrapper, onCreate } = mountList();
    await openAddRow(wrapper);

    await wrapper.get('input[aria-label="Todo にタスクを追加"]').setValue('最小のタスク');
    const save = wrapper.findAll('button').find((b) => b.text().includes('保存'));
    await save!.trigger('click');

    expect(onCreate.mock.calls).toEqual([
      [
        {
          title: '最小のタスク',
          statusId: 'status-todo',
          assigneeIds: [],
          softDeadline: null,
          priority: null,
          labelIds: [],
        },
      ],
    ]);
  });

  it('その場で決めた担当者・期限・ラベルを一緒に送る', async () => {
    const { wrapper, onCreate } = mountList();
    await openAddRow(wrapper);

    await wrapper.get('input[aria-label="Todo にタスクを追加"]').setValue('設定つきタスク');
    // 期限はアイコンを押してから入力欄が出る（参照どおりアイコンだけ並べるため）
    await wrapper.get('button[aria-label="期限を設定"]').trigger('click');
    await nextTick();
    await wrapper.get('input[aria-label="期限"]').setValue('2026-09-15');

    // 担当者とラベルはメニュー越しなので、コンポーネントの入力口を直接叩く
    const picker = wrapper.findComponent({ name: 'TaskAssigneePicker' });
    picker.vm.$emit('toggle', 'user-2', true);
    await nextTick();

    const save = wrapper.findAll('button').find((b) => b.text().includes('保存'));
    await save!.trigger('click');

    expect(onCreate).toHaveBeenCalledTimes(1);
    expect(onCreate.mock.calls[0][0]).toMatchObject({
      title: '設定つきタスク',
      statusId: 'status-todo',
      assigneeIds: ['user-2'],
      // 日付だけの入力は詳細画面と同じヘルパーで ISO へ寄せる
      softDeadline: '2026-09-15T00:00:00.000Z',
    });
  });

  it('空のタイトルでは作らず、行を閉じる', async () => {
    const { wrapper, onCreate } = mountList();
    await openAddRow(wrapper);

    const save = wrapper.findAll('button').find((b) => b.text().includes('保存'));
    // 空のときは保存を押せない（誤って空タスクを作らない）
    expect(save!.attributes('disabled')).toBeDefined();

    const cancel = wrapper.findAll('button').find((b) => b.text() === 'キャンセル');
    await cancel!.trigger('click');
    await nextTick();

    expect(onCreate).not.toHaveBeenCalled();
    expect(wrapper.find('input[aria-label="Todo にタスクを追加"]').exists()).toBe(false);
  });

  it('作成後も入力欄は開いたまま、下書きは消える（続けて足せる）', async () => {
    const { wrapper } = mountList();
    await openAddRow(wrapper);

    const input = wrapper.get('input[aria-label="Todo にタスクを追加"]');
    await input.setValue('1件目');
    const save = wrapper.findAll('button').find((b) => b.text().includes('保存'));
    await save!.trigger('click');
    await nextTick();

    expect(
      wrapper.get<HTMLInputElement>('input[aria-label="Todo にタスクを追加"]').element.value,
    ).toBe('');
  });

  it('作成に失敗したら下書きを残す（消すと打ち直しになる）', async () => {
    const { wrapper } = mountList(vi.fn(async () => false));
    await openAddRow(wrapper);

    await wrapper.get('input[aria-label="Todo にタスクを追加"]').setValue('失敗するタスク');
    const save = wrapper.findAll('button').find((b) => b.text().includes('保存'));
    await save!.trigger('click');
    await nextTick();

    expect(
      wrapper.get<HTMLInputElement>('input[aria-label="Todo にタスクを追加"]').element.value,
    ).toBe('失敗するタスク');
  });

  // 下書きは全グループで 1 組を共有している。作成中も「キャンセル」と他グループの
  // 「タスクを追加」は押せるので、完了時に無条件でリセットすると別の下書きが消える
  it('作成中に別グループで書き始めたら、そちらの下書きを消さない', async () => {
    let resolveCreate: ((created: boolean) => void) | undefined;
    const onCreate = vi.fn(
      (_input: CreateTaskInput) =>
        new Promise<boolean>((resolve) => {
          resolveCreate = resolve;
        }),
    );
    const { wrapper } = mountList(onCreate, [group, doingGroup], [status, doingStatus]);

    await openAddRow(wrapper, 'Todo');
    await wrapper.get('input[aria-label="Todo にタスクを追加"]').setValue('Todo のタスク');
    const save = sectionOf(wrapper, 'Todo')
      .findAll('button')
      .find((b) => b.text().includes('保存'));
    await save!.trigger('click');

    // 保存中に別グループへ切り替えて書き始める
    await openAddRow(wrapper, 'Doing');
    await wrapper.get('input[aria-label="Doing にタスクを追加"]').setValue('Doing の書きかけ');

    resolveCreate!(true);
    await flushPromises();

    expect(
      wrapper.get<HTMLInputElement>('input[aria-label="Doing にタスクを追加"]').element.value,
    ).toBe('Doing の書きかけ');
  });

  // 同じグループを開き直した場合も、下書きはもうその作成のものではない
  it('作成中にキャンセルして同じグループで書き直したら、その下書きを消さない', async () => {
    let resolveCreate: ((created: boolean) => void) | undefined;
    const onCreate = vi.fn(
      (_input: CreateTaskInput) =>
        new Promise<boolean>((resolve) => {
          resolveCreate = resolve;
        }),
    );
    const { wrapper } = mountList(onCreate);

    await openAddRow(wrapper);
    await wrapper.get('input[aria-label="Todo にタスクを追加"]').setValue('1 件目');
    const save = wrapper.findAll('button').find((b) => b.text().includes('保存'));
    await save!.trigger('click');

    // 保存中にキャンセルし、開き直して別の下書きを書く
    const cancel = wrapper.findAll('button').find((b) => b.text().includes('キャンセル'));
    await cancel!.trigger('click');
    await openAddRow(wrapper);
    await wrapper.get('input[aria-label="Todo にタスクを追加"]').setValue('書き直した 1 件目');

    resolveCreate!(true);
    await flushPromises();

    expect(
      wrapper.get<HTMLInputElement>('input[aria-label="Todo にタスクを追加"]').element.value,
    ).toBe('書き直した 1 件目');
  });

  it('ページの取得に失敗したら再試行を出す', async () => {
    const retry = vi.fn();
    const { wrapper } = mountList();
    await wrapper.setProps({ groups: [{ ...group, isError: true, retry }] });
    await nextTick();

    const button = wrapper.findAll('button').find((b) => b.text() === '再試行');
    expect(button).toBeDefined();
    await button!.trigger('click');
    expect(retry).toHaveBeenCalledTimes(1);
  });

  it('作成の失敗をグループの下に出す', async () => {
    const { wrapper } = mountList();
    await wrapper.setProps({ createErrors: { 'status-todo': 'タスクを作成できませんでした' } });
    await nextTick();

    expect(wrapper.text()).toContain('タスクを作成できませんでした');
  });

  // 追加行は v-for の内側にあるので、template ref が配列で入る。素の `.$el` を読むと
  // 常に undefined になり、型検査も既存テストも通ったままフォーカスだけが効かなくなる
  it('追加行を開くとタイトル欄にフォーカスが当たる', async () => {
    const { wrapper } = mountList();
    await openAddRow(wrapper);

    const input = wrapper.get<HTMLInputElement>('input[aria-label="Todo にタスクを追加"]');
    expect(document.activeElement).toBe(input.element);
  });
});
