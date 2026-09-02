import { afterEach, describe, expect, it } from 'vitest';

import { shouldCloseSidebarOnNavigate } from '../sidebar-navigation';

/** サイドバーのナビを組んで、指定要素を click の target にしたイベントを作る */
function clickOn(selector: string, init: MouseEventInit = {}) {
  document.body.innerHTML = `
    <div id="sidebar-content">
      <a id="my-tasks" href="/acme/my-tasks">My Tasks</a>
      <button id="project-row">Team Alpha</button>
      <a id="project-tasks" href="/acme/projects/ALPHA/tasks">
        <span id="link-label">タスク</span>
      </a>
      <button id="retry">再試行</button>
    </div>
  `;
  const target = document.querySelector(selector);
  if (!target) throw new Error(`target not found: ${selector}`);
  const event = new MouseEvent('click', { button: 0, bubbles: true, ...init });
  target.dispatchEvent(event);
  return event;
}

afterEach(() => {
  document.body.innerHTML = '';
});

describe('shouldCloseSidebarOnNavigate', () => {
  it('モバイルでリンクを押したら閉じる', () => {
    expect(shouldCloseSidebarOnNavigate(clickOn('#my-tasks'), true)).toBe(true);
  });

  it('リンクの中の要素を押しても閉じる', () => {
    expect(shouldCloseSidebarOnNavigate(clickOn('#link-label'), true)).toBe(true);
  });

  // デスクトップのサイドバーは常設で、ページと重ならない
  it('デスクトップでは閉じない', () => {
    expect(shouldCloseSidebarOnNavigate(clickOn('#my-tasks'), false)).toBe(false);
  });

  // プロジェクト行は展開のトグルで、ページは動かない
  it('リンクでない操作（行の展開・再試行）では閉じない', () => {
    expect(shouldCloseSidebarOnNavigate(clickOn('#project-row'), true)).toBe(false);
    expect(shouldCloseSidebarOnNavigate(clickOn('#retry'), true)).toBe(false);
  });

  it('ナビの余白を押しても閉じない', () => {
    expect(shouldCloseSidebarOnNavigate(clickOn('#sidebar-content'), true)).toBe(false);
  });

  it.each([
    ['metaKey', { metaKey: true }],
    ['ctrlKey', { ctrlKey: true }],
    ['shiftKey', { shiftKey: true }],
    ['altKey', { altKey: true }],
  ])('修飾キー付き（%s）は別タブで開くだけなので閉じない', (_label, init) => {
    expect(shouldCloseSidebarOnNavigate(clickOn('#my-tasks', init), true)).toBe(false);
  });

  it.each([
    ['中クリック', 1],
    ['右クリック', 2],
  ])('%s では閉じない', (_label, button) => {
    expect(shouldCloseSidebarOnNavigate(clickOn('#my-tasks', { button }), true)).toBe(false);
  });
});
