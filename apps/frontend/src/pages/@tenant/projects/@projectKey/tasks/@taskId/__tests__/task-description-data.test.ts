// +data.ts (タスク説明の SSR KFM 描画) の契約テスト。
// 最重要は「descriptionHtml が必ず renderDescription の出力そのものであること」——
// v-html へ入る値の唯一の供給源がサニタイズ済みサーバ描画であることの機械的な証明。
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { PageContextServer } from 'vike/types';

import { renderDescription } from '@/lib/markup-renderer';
import { data } from '../+data';

const TENANT_UUID = '11111111-2222-3333-4444-555555555555';
const PROJECT_UUID = '66666666-7777-8888-9999-aaaaaaaaaaaa';
const TASK_UUID = 'bbbbbbbb-cccc-dddd-eeee-ffffffffffff';

const DESCRIPTION = [
  '# 見出し',
  '',
  '**強調** と `code`',
  '',
  '> [!NOTE]',
  '> callout 本文',
  '',
  '脚注あり[^1]',
  '',
  '[^1]: 脚注本文',
].join('\n');

function jsonResponse(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

type FetchCall = { url: string; cookie: string | null };

/**
 * tenants → projects → task の 3 GET を成功で返す fetch モック。
 * 呼び出しの URL と cookie ヘッダを記録する。
 */
function stubBackend(overrides: { taskStatus?: number; description?: string | null } = {}) {
  const calls: FetchCall[] = [];
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = input instanceof Request ? input.url : input.toString();
      const headers = new Headers(init?.headers);
      calls.push({ url, cookie: headers.get('cookie') });
      if (url.endsWith('/v1/tenants')) {
        return jsonResponse([{ id: TENANT_UUID, display_id: 'acme' }]);
      }
      if (url.endsWith(`/v1/tenants/${TENANT_UUID}/projects`)) {
        return jsonResponse([{ id: PROJECT_UUID, key: 'ENG' }]);
      }
      if (url.endsWith(`/v1/tenants/${TENANT_UUID}/projects/${PROJECT_UUID}/tasks/ENG-42`)) {
        if (overrides.taskStatus) return jsonResponse({}, overrides.taskStatus);
        return jsonResponse({
          id: TASK_UUID,
          description: overrides.description === undefined ? DESCRIPTION : overrides.description,
        });
      }
      return jsonResponse({}, 404);
    }),
  );
  return calls;
}

function pageContext(): PageContextServer {
  return {
    routeParams: { tenant: 'acme', projectKey: 'ENG', taskId: 'ENG-42' },
    headers: { cookie: 'session=secret-cookie' },
  } as unknown as PageContextServer;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('task description +data', () => {
  it('descriptionHtml は renderDescription(text, { scope: task-<uuid> }) の出力そのもの', async () => {
    stubBackend();
    const result = await data(pageContext());
    const expected = await renderDescription(DESCRIPTION, { scope: `task-${TASK_UUID}` });
    expect(result.descriptionHtml).toBe(expected);
  });

  it('Markdown が描画され、alert は callout として出る', async () => {
    stubBackend();
    const { descriptionHtml } = await data(pageContext());
    expect(descriptionHtml).toContain('<strong>');
    expect(descriptionHtml).toContain('<code>');
    expect(descriptionHtml).toContain('kfm-alert');
    // 生テキストがそのまま出ていない (マーカー記法が残らない)
    expect(descriptionHtml).not.toContain('[!NOTE]');
    expect(descriptionHtml).not.toContain('**強調**');
  });

  it('scope は決定的: 同一入力の 2 回で同一 HTML、脚注 id に task-<uuid> prefix が付く', async () => {
    stubBackend();
    const first = await data(pageContext());
    const second = await data(pageContext());
    expect(first.descriptionHtml).toBe(second.descriptionHtml);
    expect(first.descriptionHtml).toContain(`user-content-task-${TASK_UUID}-fn-`);
  });

  it('リクエストの cookie を backend の全呼び出しへ転送する', async () => {
    const calls = stubBackend();
    await data(pageContext());
    expect(calls).toHaveLength(3);
    for (const call of calls) {
      expect(call.cookie).toBe('session=secret-cookie');
    }
  });

  it('取得失敗・説明なしは null (プレーン表示へフォールバック)', async () => {
    stubBackend({ taskStatus: 500 });
    expect((await data(pageContext())).descriptionHtml).toBeNull();

    stubBackend({ description: null });
    expect((await data(pageContext())).descriptionHtml).toBeNull();

    // fetch 自体の失敗 (backend down) でも throw しない
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        throw new Error('ECONNREFUSED');
      }),
    );
    expect((await data(pageContext())).descriptionHtml).toBeNull();
  });

  it('tenant / project が見つからない場合は null', async () => {
    stubBackend();
    const missingTenant = {
      routeParams: { tenant: 'unknown', projectKey: 'ENG', taskId: 'ENG-42' },
      headers: { cookie: 'session=secret-cookie' },
    } as unknown as PageContextServer;
    expect((await data(missingTenant)).descriptionHtml).toBeNull();

    const missingProject = {
      routeParams: { tenant: 'acme', projectKey: 'NOPE', taskId: 'ENG-42' },
      headers: { cookie: 'session=secret-cookie' },
    } as unknown as PageContextServer;
    expect((await data(missingProject)).descriptionHtml).toBeNull();
  });
});
