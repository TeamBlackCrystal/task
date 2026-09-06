/**
 * 画面をまたぐ「操作できた」通知の印。
 *
 * URL のクエリだけを根拠に成功通知を出すと、その URL を開かせるだけで通知を偽装できる。
 * `/settings/security?linked=github` を開かせれば連携していない人にも「GitHub を連携しました」が、
 * `/signin?password_changed=1` を開かせればパスワードを変えていない人にも「失効しました」が出る。
 * sessionStorage は操作した本人のタブにしか書けないので、開始する側で印を置き、戻ってきた
 * 画面で 1 回だけ消費する。
 */

/** OAuth 連携を開始したプロバイダー。値はプロバイダー名。 */
export const OAUTH_LINK_NOTICE = 'task:oauth-link-started';

/** パスワード変更でサインアウトされたこと。 */
export const PASSWORD_CHANGED_NOTICE = 'task:password-changed';

export function markNotice(key: string, value = '1'): void {
  try {
    window.sessionStorage.setItem(key, value);
  } catch {
    // sessionStorage を使えない設定では通知が出ないだけ。操作自体は止めない
  }
}

/** 印を読んで消す。2 回目以降は null になる。 */
export function consumeNotice(key: string): string | null {
  try {
    const value = window.sessionStorage.getItem(key);
    window.sessionStorage.removeItem(key);
    return value;
  } catch {
    return null;
  }
}
