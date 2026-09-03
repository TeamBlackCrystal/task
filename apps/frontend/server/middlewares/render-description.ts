/**
 * POST /internal/render-description — 説明本文の KFM 描画をサーバで行う口。
 *
 * 一覧の分割ビュー（右ペイン）は選択がクライアント操作なので、詳細ページのような
 * server data hook（`@taskId/+data.ts`）が選択中タスクに追従できない。KFM の描画を
 * クライアントへ載せると +417.5 KB になる（kfm-client-registry テスト参照）ため、
 * `/internal/password-strength` と同じ形でサーバへ本文を送り HTML だけを受け取る。
 *
 * 入力は呼び出し側が既に持っている本文なので、この口は権限の境界を動かさない
 * （他人の本文を取得する手段にはならない。描画するのは送られてきた文字列だけ）。
 *
 * ただし描画そのものは CPU を使う（remark / rehype / starry-night / sanitize）。
 * この Elysia はドメイン直下に出ているので、素のままだと誰でも SSR と同じ
 * イベントループを削れる。呼ぶのはログイン済みの画面だけなので、入口で次の 2 つを見る:
 *
 * 1. `content-length` が明らかに大きいリクエストを読む前に 413 で落とす
 *    （`api-proxy.ts` の `rejectIfContentLengthTooLarge` と同じ流儀）
 * 2. backend にセッションを確かめ、ログインしていなければ 401
 *
 * **セッション Cookie の有無では判定しない。** backend の `axum_session` は
 * `SessionMode::Persistent` の既定で動いているため、未ログインの訪問者にも
 * `session` Cookie が発行される。有無だけを見ると、一度サイトを開くか
 * API を 1 回叩くだけで通ってしまう。
 *
 * 呼び出し側（`useRenderedDescription`）は `!response.ok` を `html: null` に畳んで
 * プレーン表示へ倒すので、どちらの拒否でも表示の壊れ方は変わらない。
 */
import { Elysia, t } from 'elysia';

import { renderDescription } from '@/lib/markup-renderer';
import { API_BASE } from '../api-base';

/**
 * 本文長の上限。`@taskId/+data.ts` の MAX_DESCRIPTION_LENGTH と同じ値・同じ理由で、
 * 超過分は描画せず null を返して呼び出し側のプレーン表示へ倒す。
 * renderDescription の CPU も L1 キャッシュのメモリも本文長に比例するため、
 * 非有界の入力を描画へ入れない。
 */
export const MAX_DESCRIPTION_LENGTH = 65_536;

/**
 * 読む前に落とすリクエスト本文の上限。
 *
 * JSON のエスケープと multibyte を考えると本文長の数倍にはなるので、
 * 文字数の上限そのままでは正当なリクエストを弾いてしまう。余裕を持たせた上で、
 * Bun の既定（128 MB）をそのまま受けないための入口として使う。
 */
export const MAX_REQUEST_BODY_BYTES = MAX_DESCRIPTION_LENGTH * 8;

/** `/v1/auth/me` の結果を短時間だけ覚える。連続で開いたときに毎回問い合わせない。 */
const SESSION_CACHE_TTL_MS = 10_000;
const sessionCache = new Map<string, { authenticated: boolean; expiresAt: number }>();
/** 覚える件数の上限（無制限に持つと Cookie を変えるだけで膨らませられる）。 */
const SESSION_CACHE_MAX = 500;

function cacheSession(cookie: string, authenticated: boolean) {
  if (sessionCache.size >= SESSION_CACHE_MAX) sessionCache.clear();
  sessionCache.set(cookie, { authenticated, expiresAt: Date.now() + SESSION_CACHE_TTL_MS });
}

/**
 * backend にセッションを確かめる。
 *
 * 失敗（backend が落ちている等）は「認証できない」に倒す。ここで通してしまうと、
 * backend が不調なときだけ誰でも描画を走らせられることになる。
 */
async function isAuthenticated(cookie: string | null): Promise<boolean> {
  if (!cookie) return false;

  const cached = sessionCache.get(cookie);
  if (cached && cached.expiresAt > Date.now()) return cached.authenticated;

  try {
    const response = await fetch(`${API_BASE}/v1/auth/me`, {
      headers: { cookie },
      // 認証確認のために本体は要らない。SSR のイベントループを長く塞がない
      signal: AbortSignal.timeout(3_000),
    });
    const authenticated = response.ok;
    cacheSession(cookie, authenticated);
    return authenticated;
  } catch {
    return false;
  }
}

/** この口のパス。`onRequest` は routing より前に走るので、自分の宛先だけを見る。 */
const PATH = '/internal/render-description';

export const renderDescriptionPlugin = new Elysia()
  // content-length は routing・本文解析より前に見る。beforeHandle では本文を
  // 読み終えたあとになるので、大きいリクエストを読まずに落とす意味が無くなる
  .onRequest(({ request }) => {
    // onRequest は全リクエストを通るので、URL を組む前に安く外す（ページ表示は GET）
    if (request.method !== 'POST') return;
    if (new URL(request.url).pathname !== PATH) return;
    const contentLength = Number(request.headers.get('content-length'));
    if (Number.isFinite(contentLength) && contentLength > MAX_REQUEST_BODY_BYTES) {
      return new Response('Payload Too Large', { status: 413 });
    }
  })
  .post(
    PATH,
    async ({ body }) => {
      const { description, taskId } = body;
      if (!description) return { html: null };

      try {
        // scope はタスク UUID で決定的（同一入力 → 同一 HTML）。詳細ページの +data.ts と
        // 同じ組み立てにして、同じタスクなら両経路で脚注 id が一致するようにする。
        // UUID しか受けないので、scope の文字集合制約 [A-Za-z0-9_-]+ は常に満たす。
        return { html: await renderDescription(description, { scope: `task-${taskId}` }) };
      } catch (error) {
        // 描画失敗で 500 を返すと、呼び出し側は説明以外も含めて壊れたように見える。
        // 説明の KFM 表示は付加価値なので null へ倒し、プレーン表示を選ばせる。
        console.error('[render-description] failed', error);
        return { html: null };
      }
    },
    {
      // ログインしていないリクエストは描画へ入れない
      beforeHandle: async ({ request, status }) => {
        if (!(await isAuthenticated(request.headers.get('cookie')))) {
          return status(401, 'Unauthorized');
        }
      },
      body: t.Object({
        taskId: t.String({ format: 'uuid' }),
        // 上限超過は描画へ入れない。スキーマで縛ると本文を読んだ時点で 422 になり、
        // renderDescription まで到達しない（呼び出し側は null へ畳む）
        description: t.String({ maxLength: MAX_DESCRIPTION_LENGTH }),
      }),
    },
  );
