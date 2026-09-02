import { afterEach, describe, expect, it } from 'vitest';
import { enableAutoUnmount, mount } from '@vue/test-utils';
import { nextTick } from 'vue';

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
  it('href は詳細ページ（ディープリンク・新しいタブで開ける）', () => {
    const wrapper = mountLink();
    expect(wrapper.get('a').attributes('href')).toBe('/acme/projects/ENG/tasks/ENG-42');
  });

  // 分割ビューに出すか詳細ページへ送るかは呼び出し側がクリック時に決める。
  // ここで真偽値の prop として受け取ると描画時の値が固まり、画面幅の判定が
  // 古いまま残る（本番で「右ペインは出ているのに一覧のクリックは遷移する」が起きた）
  it('素の左クリックは既定動作を止めて select を emit する', async () => {
    const wrapper = mountLink();

    // preventDefault が落ちるとブラウザが href を辿ってフルページ遷移し、同時に親が
    // selectedTaskId を立てる。この PR が直した症状そのものが戻るので、emit だけでなく
    // 既定動作を止めたことも見る。trigger では defaultPrevented を読めないので自分で組む
    // （cancelable が無いと preventDefault が効かず、この検証が常に通ってしまう）
    const event = new MouseEvent('click', { button: 0, bubbles: true, cancelable: true });
    wrapper.get('a').element.dispatchEvent(event);
    await nextTick();

    expect(event.defaultPrevented).toBe(true);
    expect(wrapper.emitted('select')).toEqual([[42]]);
  });

  it.each([
    ['metaKey', { metaKey: true }],
    ['ctrlKey', { ctrlKey: true }],
    ['shiftKey', { shiftKey: true }],
    ['altKey', { altKey: true }],
  ])('%s 付きのクリックは select を出さず href（フルページ）に委ねる', async (_label, mods) => {
    const wrapper = mountLink();

    const event = new MouseEvent('click', { button: 0, bubbles: true, cancelable: true, ...mods });
    wrapper.get('a').element.dispatchEvent(event);
    await nextTick();

    // 止めてしまうと修飾キーでも新しいタブが開かなくなる
    expect(event.defaultPrevented).toBe(false);
    expect(wrapper.emitted('select')).toBeFalsy();
  });

  it('左クリック以外は select を出さない', async () => {
    const wrapper = mountLink();
    await wrapper.get('a').trigger('click', { button: 1 });
    expect(wrapper.emitted('select')).toBeFalsy();
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
});
