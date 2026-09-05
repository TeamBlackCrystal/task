import { afterEach, describe, expect, it, vi } from 'vitest';
import { enableAutoUnmount, mount } from '@vue/test-utils';

import TaskAssigneePicker from '@/components/tasks/TaskAssigneePicker.vue';

enableAutoUnmount(afterEach);

const members = [
  { id: 'user-1', username: 'yupix', avatar_url: null },
  { id: 'user-2', username: 'sousuke', avatar_url: null },
];

/**
 * メニューの中身はトリガーを押さないと描画されないので、
 * DropdownMenuContent を素の div に差し替えて中を読む。
 */
function mountPicker(props: Record<string, unknown>) {
  return mount(TaskAssigneePicker, {
    props: { members: [], selected: [], ...props },
    global: {
      stubs: {
        DropdownMenu: { template: '<div><slot /></div>' },
        DropdownMenuTrigger: { template: '<div><slot /></div>' },
        DropdownMenuContent: { template: '<div><slot /></div>' },
        DropdownMenuCheckboxItem: { template: '<div><slot /></div>' },
      },
    },
  });
}

// 取得中・失敗を「候補が 0 人」と混ぜると、権限や通信で候補が取れていないのに
// 「いません」と出て、担当者を触れない理由が分からなくなる
describe('TaskAssigneePicker の候補の取得状態', () => {
  it('取得中は読み込み中を出す', () => {
    const wrapper = mountPicker({ membersState: { loading: true } });

    expect(wrapper.text()).toContain('読み込み中…');
    expect(wrapper.text()).not.toContain('担当者に指定できる利用者がいません');
  });

  it('失敗は失敗として出し、再試行できる', async () => {
    const onRetry = vi.fn();
    const wrapper = mountPicker({ membersState: { error: true, onRetry } });

    expect(wrapper.text()).toContain('候補を読み込めませんでした');
    expect(wrapper.text()).not.toContain('担当者に指定できる利用者がいません');

    const button = wrapper.findAll('button').find((b) => b.text() === '再試行');
    expect(button).toBeDefined();
    await button!.trigger('click');
    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it('取得できて 0 人のときだけ「いません」を出す', () => {
    const wrapper = mountPicker({ membersState: { loading: false, error: false } });

    expect(wrapper.text()).toContain('担当者に指定できる利用者がいません');
  });

  it('候補があれば絞り込める', async () => {
    const wrapper = mountPicker({ members });

    expect(wrapper.text()).toContain('yupix');
    expect(wrapper.text()).toContain('sousuke');

    await wrapper.get('input[aria-label="メンバーを検索"]').setValue('sou');

    expect(wrapper.text()).not.toContain('yupix');
    expect(wrapper.text()).toContain('sousuke');
  });
});
