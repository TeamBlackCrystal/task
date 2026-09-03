// @vitest-environment node
import { Elysia } from 'elysia';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  MAX_DESCRIPTION_LENGTH,
  MAX_REQUEST_BODY_BYTES,
  renderDescriptionPlugin,
} from '../render-description';

const app = new Elysia().use(renderDescriptionPlugin);

const TASK_UUID = '00000000-0000-0000-0000-000000000042';

/**
 * 入口の認証確認（backend の `/v1/auth/me`）を差し替える。
 *
 * Cookie は毎回変える。プラグインは短時間だけ判定を覚えるので、
 * 同じ値を使い回すとテスト間で前の結果を引く。
 */
let authOk = true;
let cookieSeq = 0;
let authCalls: (string | null)[] = [];

beforeEach(() => {
  authOk = true;
  authCalls = [];
  vi.stubGlobal('fetch', async (input: string | URL | Request, init?: RequestInit) => {
    const url = String(input instanceof Request ? input.url : input);
    if (url.endsWith('/v1/auth/me')) {
      authCalls.push((init?.headers as Record<string, string> | undefined)?.cookie ?? null);
      return new Response(authOk ? '{"id":"user-1"}' : '{"message":"unauthorized"}', {
        status: authOk ? 200 : 401,
        headers: { 'Content-Type': 'application/json' },
      });
    }
    throw new Error(`unexpected fetch: ${url}`);
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

function freshCookie() {
  cookieSeq += 1;
  return `session=test-session-${cookieSeq}`;
}

async function post(
  body: unknown,
  options: { cookie?: string | null; contentLength?: number } = {},
): Promise<Response> {
  const headers: Record<string, string> = { 'Content-Type': 'application/json' };
  const cookie = options.cookie === undefined ? freshCookie() : options.cookie;
  if (cookie) headers.cookie = cookie;
  if (options.contentLength !== undefined) {
    headers['content-length'] = String(options.contentLength);
  }
  return app.handle(
    new Request('http://localhost/internal/render-description', {
      method: 'POST',
      headers,
      body: JSON.stringify(body),
    }),
  );
}

describe('POST /internal/render-description', () => {
  it('markdown を KFM の HTML にして返す', async () => {
    const response = await post({ taskId: TASK_UUID, description: '# 見出し\n\n- 箇条書き' });
    expect(response.status).toBe(200);

    const { html } = (await response.json()) as { html: string | null };
    // 素の markdown がそのまま出ていたのが直したい症状なので、記法が要素になっていることを見る
    expect(html).toContain('<h1');
    expect(html).toContain('<li>');
    expect(html).not.toContain('# 見出し');
  });

  it('脚注の id はタスク UUID で scope 化する（詳細ページと同じ組み立て）', async () => {
    const response = await post({
      taskId: TASK_UUID,
      description: '本文[^1]\n\n[^1]: 注記',
    });

    const { html } = (await response.json()) as { html: string | null };
    // 同一ページに複数の KFM 断片が並んだときに id が衝突しないための scope
    expect(html).toContain(`user-content-task-${TASK_UUID}-`);
  });

  it('空の本文は描画せず null を返す', async () => {
    const response = await post({ taskId: TASK_UUID, description: '' });

    expect(response.status).toBe(200);
    expect(await response.json()).toEqual({ html: null });
  });

  it('上限ちょうどの本文は描画する', async () => {
    const response = await post({
      taskId: TASK_UUID,
      description: 'a'.repeat(MAX_DESCRIPTION_LENGTH),
    });

    const { html } = (await response.json()) as { html: string | null };
    expect(html).toContain('<p>');
  });

  it('UUID でない taskId は受け付けない（scope の文字集合を壊さない）', async () => {
    const response = await post({ taskId: 'task-1 onerror=x', description: '本文' });

    expect(response.status).toBeGreaterThanOrEqual(400);
  });

  it('script は sanitize で落とす', async () => {
    const response = await post({
      taskId: TASK_UUID,
      description: '<script>alert(1)</script>\n\n本文',
    });

    const { html } = (await response.json()) as { html: string | null };
    expect(html).not.toContain('<script');
  });
});

// この Elysia はドメイン直下に出ているので、素のままだと誰でも
// remark / rehype / starry-night / sanitize の一式を SSR と同じイベントループで走らせられる。
// 呼ぶのはログイン済みの画面だけなので、入口で落とす
describe('POST /internal/render-description の入口', () => {
  it('Cookie が無ければ描画せず 401', async () => {
    const response = await post({ taskId: TASK_UUID, description: '# 見出し' }, { cookie: null });

    expect(response.status).toBe(401);
    // backend へ問い合わせるまでもない
    expect(authCalls).toEqual([]);
  });

  it('ログインしていなければ 401', async () => {
    authOk = false;
    const response = await post({ taskId: TASK_UUID, description: '# 見出し' });

    expect(response.status).toBe(401);
    expect(authCalls).toHaveLength(1);
  });

  // Cookie の有無で判定してはいけない: backend の axum_session は Persistent 既定で、
  // 未ログインの訪問者にも session Cookie を発行する
  it('Cookie があってもセッションを backend に確かめる', async () => {
    const cookie = freshCookie();
    await post({ taskId: TASK_UUID, description: '本文' }, { cookie });

    expect(authCalls).toEqual([cookie]);
  });

  it('同じ Cookie の連続呼び出しでは確認を繰り返さない', async () => {
    const cookie = freshCookie();
    await post({ taskId: TASK_UUID, description: '1 回目' }, { cookie });
    await post({ taskId: TASK_UUID, description: '2 回目' }, { cookie });

    expect(authCalls).toHaveLength(1);
  });

  it('backend に届かないときは通さない', async () => {
    vi.stubGlobal('fetch', async () => {
      throw new Error('backend down');
    });

    const response = await post({ taskId: TASK_UUID, description: '# 見出し' });

    expect(response.status).toBe(401);
  });

  it('content-length が上限を超えるリクエストは本文を読まずに 413', async () => {
    const response = await post(
      { taskId: TASK_UUID, description: '本文' },
      { contentLength: MAX_REQUEST_BODY_BYTES + 1 },
    );

    expect(response.status).toBe(413);
    // 認証確認より前に落とす（大きい入力に backend への往復を足さない）
    expect(authCalls).toEqual([]);
  });

  // 本文が読めない形でも 413 になることで、「解析より前に落ちている」ことが分かる。
  // beforeHandle で見ると本文を読み終えたあとなので、ここは 422 になる
  it('本文を解析する前に 413 で落ちている', async () => {
    const response = await app.handle(
      new Request('http://localhost/internal/render-description', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          cookie: freshCookie(),
          'content-length': String(MAX_REQUEST_BODY_BYTES + 1),
        },
        body: 'これは JSON ではない',
      }),
    );

    expect(response.status).toBe(413);
  });

  it('上限の範囲内なら content-length では落とさない', async () => {
    const response = await post(
      { taskId: TASK_UUID, description: '本文' },
      { contentLength: MAX_REQUEST_BODY_BYTES },
    );

    expect(response.status).toBe(200);
  });

  it('上限を超える本文は描画に到達しない', async () => {
    const response = await post({
      taskId: TASK_UUID,
      description: 'a'.repeat(MAX_DESCRIPTION_LENGTH + 1),
    });

    // 呼び出し側は !response.ok を html: null に畳むので、表示は今と変わらない
    expect(response.ok).toBe(false);
    expect(response.status).toBeGreaterThanOrEqual(400);
  });
});
