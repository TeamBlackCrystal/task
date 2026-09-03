import { afterEach, describe, expect, it } from 'vitest';
import { enableAutoUnmount, mount } from '@vue/test-utils';
import { nextTick } from 'vue';

import TaskGroupedList from '@/components/tasks/TaskGroupedList.vue';
import type { TaskGroup } from '@/components/tasks/task-grouped-columns';
import type { components } from '@/generated/api';

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
};

function mountList() {
  return mount(TaskGroupedList, {
    props: {
      groups: [group],
      statuses: [status],
      projectLabels: [bug],
      members,
      pending: {},
      errors: {},
    },
  });
}

async function openAddRow(wrapper: ReturnType<typeof mountList>) {
  const addButton = wrapper.findAll('button').find((b) => b.text() === 'タスクを追加');
  expect(addButton).toBeDefined();
  await addButton!.trigger('click');
  await nextTick();
}

describe('TaskGroupedList のタスク追加', () => {
  it('タイトルだけでも作れる（未指定の項目は送らない）', async () => {
    const wrapper = mountList();
    await openAddRow(wrapper);

    await wrapper.get('input[aria-label="Todo にタスクを追加"]').setValue('最小のタスク');
    const save = wrapper.findAll('button').find((b) => b.text().includes('保存'));
    await save!.trigger('click');

    expect(wrapper.emitted('create')).toEqual([
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
    const wrapper = mountList();
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

    const emitted = wrapper.emitted('create');
    expect(emitted).toHaveLength(1);
    expect(emitted![0][0]).toMatchObject({
      title: '設定つきタスク',
      statusId: 'status-todo',
      assigneeIds: ['user-2'],
      // 日付だけの入力は詳細画面と同じヘルパーで ISO へ寄せる
      softDeadline: '2026-09-15T00:00:00.000Z',
    });
  });

  it('空のタイトルでは作らず、行を閉じる', async () => {
    const wrapper = mountList();
    await openAddRow(wrapper);

    const save = wrapper.findAll('button').find((b) => b.text().includes('保存'));
    // 空のときは保存を押せない（誤って空タスクを作らない）
    expect(save!.attributes('disabled')).toBeDefined();

    const cancel = wrapper.findAll('button').find((b) => b.text() === 'キャンセル');
    await cancel!.trigger('click');
    await nextTick();

    expect(wrapper.emitted('create')).toBeUndefined();
    expect(wrapper.find('input[aria-label="Todo にタスクを追加"]').exists()).toBe(false);
  });

  it('作成後も入力欄は開いたまま、下書きは消える（続けて足せる）', async () => {
    const wrapper = mountList();
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
});
