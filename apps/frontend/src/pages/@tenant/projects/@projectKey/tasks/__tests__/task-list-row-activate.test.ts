import { afterEach, describe, expect, it } from 'vitest';

import { shouldActivateRow } from '../task-list-row-activate';

/** 行のマークアップを組んで、指定要素を click の target にしたイベントを作る */
function clickOn(selector: string, init: MouseEventInit = {}) {
  document.body.innerHTML = `
    <div id="table-container">
      <table>
        <tbody>
          <tr id="row">
            <td><button role="checkbox" id="checkbox">選択</button></td>
            <td id="key-cell"><span id="key">ENG-42</span></td>
            <td><a id="title" href="/acme/projects/ENG/tasks/ENG-42">タイトル</a></td>
            <td id="assignee-cell"><span id="avatar">y</span></td>
          </tr>
        </tbody>
      </table>
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

describe('shouldActivateRow', () => {
  it('セル内の非操作要素を押したら行を遷移させる', () => {
    expect(shouldActivateRow(clickOn('#key'))).toBe(true);
    expect(shouldActivateRow(clickOn('#avatar'))).toBe(true);
  });

  it('セルそのものを押しても行を遷移させる（余白のタップ）', () => {
    expect(shouldActivateRow(clickOn('#key-cell'))).toBe(true);
    expect(shouldActivateRow(clickOn('#assignee-cell'))).toBe(true);
  });

  // タイトルは自前の click ハンドラで遷移と修飾キーを扱うので、行側で二重に処理しない
  it('タイトルのリンクを押したら行では処理しない', () => {
    expect(shouldActivateRow(clickOn('#title'))).toBe(false);
  });

  it('チェックボックスを押したら行では処理しない', () => {
    expect(shouldActivateRow(clickOn('#checkbox'))).toBe(false);
  });

  it.each([
    ['metaKey', { metaKey: true }],
    ['ctrlKey', { ctrlKey: true }],
    ['shiftKey', { shiftKey: true }],
    ['altKey', { altKey: true }],
  ])('修飾キー付き（%s）は行では処理せず、ブラウザ本来の動作に任せる', (_label, init) => {
    expect(shouldActivateRow(clickOn('#key', init))).toBe(false);
  });

  it.each([
    ['中クリック', 1],
    ['右クリック', 2],
  ])('%s は行では処理しない', (_label, button) => {
    expect(shouldActivateRow(clickOn('#key', { button }))).toBe(false);
  });
});
