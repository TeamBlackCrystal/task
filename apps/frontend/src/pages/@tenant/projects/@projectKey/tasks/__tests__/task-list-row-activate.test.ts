import { afterEach, describe, expect, it } from 'vitest';

import { shouldActivateRow, shouldOpenRowInNewTab } from '../task-list-row-activate';

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

/**
 * 疑似要素で行全体を覆っていた間は、行のどこでも実リンクへの操作だったので
 * Ctrl / Cmd + クリックと中クリックで別タブに開けた。判定を click へ移した分を行側で補う。
 */
describe('shouldOpenRowInNewTab', () => {
  it.each([
    ['metaKey', { metaKey: true }],
    ['ctrlKey', { ctrlKey: true }],
    ['shiftKey', { shiftKey: true }],
  ])('タイトル以外のセルでも 修飾キー（%s）+ 左クリックで別タブに開く', (_label, init) => {
    expect(shouldOpenRowInNewTab(clickOn('#key', init))).toBe(true);
    expect(shouldOpenRowInNewTab(clickOn('#assignee-cell', init))).toBe(true);
  });

  it('中クリックでも別タブに開く', () => {
    expect(shouldOpenRowInNewTab(clickOn('#key', { button: 1 }))).toBe(true);
  });

  // ダウンロードの合図に使うブラウザがあり、別タブを開く操作ではない
  it('Alt + クリックでは開かない', () => {
    expect(shouldOpenRowInNewTab(clickOn('#key', { altKey: true }))).toBe(false);
  });

  it('素の左クリックでは開かない（通常の遷移に任せる）', () => {
    expect(shouldOpenRowInNewTab(clickOn('#key'))).toBe(false);
  });

  it('右クリックでは開かない（コンテキストメニュー）', () => {
    expect(shouldOpenRowInNewTab(clickOn('#key', { button: 2 }))).toBe(false);
  });

  it('行内の操作要素では開かない（その要素本来の動作に任せる）', () => {
    expect(shouldOpenRowInNewTab(clickOn('#title', { ctrlKey: true }))).toBe(false);
    expect(shouldOpenRowInNewTab(clickOn('#checkbox', { button: 1 }))).toBe(false);
  });
});
