import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { enableAutoUnmount, mount } from '@vue/test-utils';

const { navigateSpy } = vi.hoisted(() => ({ navigateSpy: vi.fn() }));

vi.mock('vike/client/router', () => ({
  navigate: navigateSpy,
}));

import TaskTitleLink from '../TaskTitleLink.vue';

enableAutoUnmount(afterEach);

function mountLink(props: Record<string, unknown> = {}) {
  return mount(TaskTitleLink, {
    props: {
      tenantDisplayId: 'acme',
      projectKey: 'ENG',
      seqId: 42,
      title: 'タイトル',
      ...props,
    },
  });
}

describe('TaskTitleLink', () => {
  beforeEach(() => {
    navigateSpy.mockReset();
  });

  it('inlineSelect 無し: 素の左クリックで詳細ページへ navigate する', async () => {
    const wrapper = mountLink();
    await wrapper.get('a').trigger('click', { button: 0 });
    expect(navigateSpy).toHaveBeenCalledTimes(1);
    expect(navigateSpy).toHaveBeenCalledWith('/acme/projects/ENG/tasks/ENG-42');
    expect(wrapper.emitted('select')).toBeFalsy();
  });

  it('inlineSelect 有り: 素の左クリックで select を emit し navigate しない', async () => {
    const wrapper = mountLink({ inlineSelect: true });
    await wrapper.get('a').trigger('click', { button: 0 });
    expect(navigateSpy).not.toHaveBeenCalled();
    expect(wrapper.emitted('select')).toEqual([[42]]);
  });

  /**
   * 当たり判定を行全体へ広げる疑似要素を持たせてはいけない。
   * `<tr>` は WebKit で絶対配置の包含ブロックにならないため、判定が行を飛び越えて
   * テーブル全体へ広がり、全行が重なって「どこをタップしても一番下の行が開く」状態に
   * なる。行全体の当たり判定は一覧側（TableRow の click）が持つ。
   *
   * CSS の包含ブロックの問題そのものは jsdom では再現できないので、ここでは
   * 原因になったクラスが戻ってこないことだけを固定する。
   */
  it('当たり判定を行全体へ広げる疑似要素を持たない', () => {
    const wrapper = mountLink();
    const className = wrapper.get('a').classes().join(' ');
    expect(className).not.toContain('after:absolute');
    expect(className).not.toContain('after:inset-0');
  });

  it('修飾キー付きクリックは navigate も select もせず href（フルページ）に委ねる', async () => {
    const wrapper = mountLink({ inlineSelect: true });
    await wrapper.get('a').trigger('click', { button: 0, metaKey: true });
    expect(navigateSpy).not.toHaveBeenCalled();
    expect(wrapper.emitted('select')).toBeFalsy();
  });
});
