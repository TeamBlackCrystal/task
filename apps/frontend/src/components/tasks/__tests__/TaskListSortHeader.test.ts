import { afterEach, describe, expect, it } from 'vitest';
import { enableAutoUnmount, mount } from '@vue/test-utils';
import { defineComponent } from 'vue';

import TaskListSortHeader from '@/components/tasks/TaskListSortHeader.vue';
import { TASK_LIST_SORT_COLUMNS } from '@/components/tasks/task-list-sort';

enableAutoUnmount(afterEach);

const Passthrough = defineComponent({
  template: '<div><slot /></div>',
});
const RadioGroupStub = defineComponent({
  name: 'DropdownMenuRadioGroup',
  props: { modelValue: { type: String, default: '' } },
  emits: ['update:modelValue'],
  template: '<div><slot /></div>',
});
const MenuItemStub = defineComponent({
  name: 'DropdownMenuItem',
  props: { disabled: Boolean },
  emits: ['select'],
  template:
    '<button type="button" :disabled="disabled" @click="$emit(\'select\')"><slot /></button>',
});

function mountHeader(sorting: { id: string; desc: boolean }[] = []) {
  return mount(TaskListSortHeader, {
    props: {
      column: TASK_LIST_SORT_COLUMNS[0],
      sorting,
    },
    global: {
      stubs: {
        DropdownMenu: Passthrough,
        DropdownMenuTrigger: Passthrough,
        DropdownMenuContent: Passthrough,
        DropdownMenuRadioGroup: RadioGroupStub,
        DropdownMenuRadioItem: Passthrough,
        DropdownMenuItem: MenuItemStub,
        DropdownMenuSeparator: true,
      },
    },
  });
}

describe('TaskListSortHeader', () => {
  it('昇順・降順を選ぶと単一列の並びを通知する', () => {
    const wrapper = mountHeader();
    const group = wrapper.findComponent(RadioGroupStub);

    group.vm.$emit('update:modelValue', 'asc');
    group.vm.$emit('update:modelValue', 'desc');

    expect(wrapper.emitted('update:sorting')).toEqual([
      [[{ id: 'title', desc: false }]],
      [[{ id: 'title', desc: true }]],
    ]);
  });

  it('有効な並びをトリガーに示し、クリアできる', async () => {
    const wrapper = mountHeader([{ id: 'title', desc: true }]);

    expect(wrapper.get('button').attributes('aria-label')).toBe(
      'タスクを並べ替え、現在は名前の降順',
    );
    const clear = wrapper.findComponent(MenuItemStub);
    expect(clear.props('disabled')).toBe(false);
    clear.vm.$emit('select');

    expect(wrapper.emitted('update:sorting')).toEqual([[[]]]);
  });

  it('別の列が有効なときはクリア操作を無効にする', () => {
    const wrapper = mountHeader([{ id: 'priority', desc: false }]);

    expect(wrapper.get('button').attributes('aria-label')).toBe('タスクを並べ替え');
    expect(wrapper.findComponent(MenuItemStub).props('disabled')).toBe(true);
  });
});
