import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest';
import {
  forgetSelectToken,
  stashSelectTokenFromUrl,
  takeSelectToken,
} from '../github-select-token';

const PROJECT_ID = '00000000-0000-4000-8000-000000000010';
const OTHER_PROJECT_ID = '00000000-0000-4000-8000-000000000020';
const SETTINGS_PATH = '/koyori/projects/TASK/settings';

function visit(path: string, fragment?: string) {
  window.history.replaceState({}, '', `${path}?section=integrations${fragment ?? ''}`);
}

/** sessionStorage が使えないとき用のメモリ退避も、テスト間で持ち越さない */
function clearStashes() {
  window.sessionStorage.clear();
  forgetSelectToken(PROJECT_ID);
  forgetSelectToken(OTHER_PROJECT_ID);
}

beforeEach(() => {
  clearStashes();
  visit(SETTINGS_PATH);
});

afterEach(() => {
  vi.restoreAllMocks();
  clearStashes();
});

describe('stashSelectTokenFromUrl', () => {
  it('フラグメントのトークンを退避し、URL から落とす', () => {
    visit(SETTINGS_PATH, '#github_select=token-1');

    stashSelectTokenFromUrl();

    expect(window.location.hash).toBe('');
    expect(takeSelectToken(PROJECT_ID)).toBe('token-1');
  });

  it('他の断片と同居していても、トークンだけを抜いて残りは保つ', () => {
    visit(SETTINGS_PATH, '#foo=bar&github_select=token-1');

    stashSelectTokenFromUrl();

    expect(window.location.hash).toBe('#foo=bar');
    expect(takeSelectToken(PROJECT_ID)).toBe('token-1');
  });

  it('フラグメントが無いときは URL も退避先も触らない', () => {
    visit(SETTINGS_PATH, '#foo=bar');

    stashSelectTokenFromUrl();

    expect(window.location.hash).toBe('#foo=bar');
    expect(takeSelectToken(PROJECT_ID)).toBeNull();
  });
});

describe('takeSelectToken', () => {
  it('退避が無ければ null を返す', () => {
    expect(takeSelectToken(PROJECT_ID)).toBeNull();
  });

  it('引き取ったあとは、同じプロジェクトなら何度でも読める（セクション切り替えで破棄されるため）', () => {
    visit(SETTINGS_PATH, '#github_select=token-1');
    stashSelectTokenFromUrl();

    expect(takeSelectToken(PROJECT_ID)).toBe('token-1');
    expect(takeSelectToken(PROJECT_ID)).toBe('token-1');
  });

  it('着地ページと違うパスでは引き取らない', () => {
    visit(SETTINGS_PATH, '#github_select=token-1');
    stashSelectTokenFromUrl();

    visit('/koyori/projects/OTHER/settings');
    expect(takeSelectToken(OTHER_PROJECT_ID)).toBeNull();
  });

  it('引き取らなかった退避は捨てる（別プロジェクトへ紛れ込ませない）', () => {
    visit(SETTINGS_PATH, '#github_select=token-1');
    stashSelectTokenFromUrl();

    // パス不一致でいちど弾かれたら、着地ページへ戻っても復活しない
    visit('/koyori/projects/OTHER/settings');
    expect(takeSelectToken(OTHER_PROJECT_ID)).toBeNull();

    visit(SETTINGS_PATH);
    expect(takeSelectToken(PROJECT_ID)).toBeNull();
  });

  it('壊れた退避を読んでも例外にしない', () => {
    window.sessionStorage.setItem('github-select-token:pending', '{壊れた JSON');

    expect(takeSelectToken(PROJECT_ID)).toBeNull();
  });
});

/**
 * トークンは退避したあと URL から落とすので、sessionStorage へ書けなかったときに
 * 何も残らないと唯一の控えを失い、選択 UI が出ないまま連携できなくなる。
 */
describe('sessionStorage が使えない環境', () => {
  it('書き込みが例外でも、URL から落としたトークンを引き取れる', () => {
    const setItem = vi.spyOn(window.sessionStorage, 'setItem').mockImplementation(() => {
      throw new Error('QuotaExceededError');
    });
    visit(SETTINGS_PATH, '#github_select=token-1');

    stashSelectTokenFromUrl();

    expect(setItem).toHaveBeenCalled();
    expect(window.location.hash).toBe('');
    expect(takeSelectToken(PROJECT_ID)).toBe('token-1');
  });

  it('読み書きの両方が例外でも引き取れる', () => {
    vi.spyOn(window.sessionStorage, 'setItem').mockImplementation(() => {
      throw new Error('access denied');
    });
    vi.spyOn(window.sessionStorage, 'getItem').mockImplementation(() => {
      throw new Error('access denied');
    });
    visit(SETTINGS_PATH, '#github_select=token-1');

    stashSelectTokenFromUrl();

    expect(takeSelectToken(PROJECT_ID)).toBe('token-1');
  });

  it('着地ページと違うパスなら、この環境でも引き取らない', () => {
    vi.spyOn(window.sessionStorage, 'setItem').mockImplementation(() => {
      throw new Error('access denied');
    });
    visit(SETTINGS_PATH, '#github_select=token-1');
    stashSelectTokenFromUrl();

    visit('/koyori/projects/OTHER/settings');
    expect(takeSelectToken(OTHER_PROJECT_ID)).toBeNull();
  });

  it('捨てたトークンはメモリの退避からも消える', () => {
    vi.spyOn(window.sessionStorage, 'setItem').mockImplementation(() => {
      throw new Error('access denied');
    });
    visit(SETTINGS_PATH, '#github_select=token-1');
    stashSelectTokenFromUrl();
    expect(takeSelectToken(PROJECT_ID)).toBe('token-1');

    forgetSelectToken(PROJECT_ID);

    expect(takeSelectToken(PROJECT_ID)).toBeNull();
  });
});

describe('forgetSelectToken', () => {
  it('引き取り済みのトークンと未引き取りの退避を両方捨てる', () => {
    visit(SETTINGS_PATH, '#github_select=token-1');
    stashSelectTokenFromUrl();
    expect(takeSelectToken(PROJECT_ID)).toBe('token-1');

    forgetSelectToken(PROJECT_ID);

    expect(takeSelectToken(PROJECT_ID)).toBeNull();
  });

  it('別プロジェクトのトークンは消さない', () => {
    visit(SETTINGS_PATH, '#github_select=token-1');
    stashSelectTokenFromUrl();
    expect(takeSelectToken(PROJECT_ID)).toBe('token-1');

    forgetSelectToken(OTHER_PROJECT_ID);

    expect(takeSelectToken(PROJECT_ID)).toBe('token-1');
  });
});
