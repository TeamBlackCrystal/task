/**
 * GitHub App インストール callback が渡すリポジトリ選択トークンの受け取り。
 *
 * backend はトークンを単独のフラグメント（`#github_select=...`）で返す。
 * クエリだと frontend / CDN のアクセスログと Referer に残るためで、その代わり
 * フラグメントはサーバーへ送られず Vike の pageContext にも現れない。
 * ハイドレーション時の history 書き換えで URL から落ちるため、読み取りが
 * 遅れると失われる。
 *
 * 設定ページはテナント / プロジェクトの ID を API で解決し終わるまで連携
 * セクションをマウントしないので、セクションの `onMounted` はハイドレーション
 * より数往復あとになる。そこで消える前に Vike の client entry
 * （`src/pages/+client.ts`）で退避し、セクションは退避先から引き取る。
 */

const STORAGE_KEY_PREFIX = 'github-select-token';

/** プロジェクト ID が判明する前の一時退避先（client entry が書き、セクションが引き取る） */
const PENDING_KEY = `${STORAGE_KEY_PREFIX}:pending`;

const FRAGMENT_PARAM = 'github_select';

/**
 * 退避したトークンと、それを受け取ったページのパス。
 *
 * パスを添えるのは、引き取り先を着地ページに限るため。プロジェクト ID は
 * client entry の時点では分からないので、パスで照合する。
 */
type PendingStash = { token: string; path: string };

function projectKey(projectId: string) {
  return `${STORAGE_KEY_PREFIX}:${projectId}`;
}

/** sessionStorage はプライベートモードや権限設定で触れないことがある */
function readStorage(key: string): string | null {
  try {
    return window.sessionStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeStorage(key: string, value: string) {
  try {
    window.sessionStorage.setItem(key, value);
  } catch {
    // 退避できなくても致命的ではない（連携をやり直せば済む）
  }
}

function removeStorage(key: string) {
  try {
    window.sessionStorage.removeItem(key);
  } catch {
    // 同上
  }
}

/**
 * フラグメントのトークンを退避し、URL から落とす。
 *
 * URL に残すと履歴とブラウザのアドレスバーにトークンが残り、リロードで
 * 期限切れのトークンが蘇る。ハイドレーションより前に呼ぶこと。
 */
export function stashSelectTokenFromUrl() {
  if (typeof window === 'undefined') return;

  const url = new URL(window.location.href);
  const hash = new URLSearchParams(url.hash.slice(1));
  const token = hash.get(FRAGMENT_PARAM);
  if (!token) return;

  const stash: PendingStash = { token, path: url.pathname };
  writeStorage(PENDING_KEY, JSON.stringify(stash));

  // backend はトークンを単独のフラグメントで返すが、他の断片と同居していても
  // それだけを抜いて残りは保つ。
  hash.delete(FRAGMENT_PARAM);
  const rest = hash.toString();
  url.hash = rest ? `#${rest}` : '';
  window.history.replaceState(window.history.state, '', url);
}

/**
 * このプロジェクト向けのトークンを取り出す。
 *
 * 一時退避されたものは、着地ページと同じパスで開かれたときだけ引き取り、
 * 以降はプロジェクト単位の退避先へ移す（設定セクションを切り替えると
 * セクションは破棄されるため、タブ内で保持し続ける必要がある）。
 */
export function takeSelectToken(projectId: string): string | null {
  if (typeof window === 'undefined') return null;

  const own = readStorage(projectKey(projectId));
  if (own) return own;

  const raw = readStorage(PENDING_KEY);
  if (!raw) return null;

  // 引き取りは 1 回だけ。パスが違って引き取らなかった分も、あとで別プロジェクトに
  // 紛れ込まないよう捨てる（トークンは backend 側も 10 分で切れる）。
  removeStorage(PENDING_KEY);

  let stash: PendingStash;
  try {
    stash = JSON.parse(raw) as PendingStash;
  } catch {
    return null;
  }
  if (typeof stash?.token !== 'string' || stash.path !== window.location.pathname) return null;

  writeStorage(projectKey(projectId), stash.token);
  return stash.token;
}

/** 使い終わった（または無効になった）トークンを捨てる */
export function forgetSelectToken(projectId: string) {
  if (typeof window === 'undefined') return;
  removeStorage(projectKey(projectId));
  removeStorage(PENDING_KEY);
}
