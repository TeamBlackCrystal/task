// タスク詳細ページの server data hook (https://vike.dev/data)。
// KFM の SSR 契約 (@/lib/markup-renderer/index.ts) の消費側:
// renderDescription はここ (サーバ) で一度だけ実行し、コンポーネントは
// descriptionHtml を v-html で受けるだけにする。クライアント再パースはしない
// (markup-renderer を client へ載せると +417.5 KB — kfm-client-registry テスト参照)。
// +data は既定で server-only のため、この import が client バンドルへ入ることはない。
import type { PageContextServer } from 'vike/types';

import type { components } from '@/generated/api';
import { renderDescription } from '@/lib/markup-renderer';

type TenantResponse = components['schemas']['TenantResponse'];
type ProjectResponse = components['schemas']['ProjectResponse'];
type TaskDetail = components['schemas']['TaskDetailResponse'];

// server/middlewares/api-proxy.ts の env.API_BASE と同じ既定値。+data は Vike の
// server ランタイムで走り Elysia の proxy を経由しないため、backend を直接叩く。
const API_BASE = process.env.API_BASE ?? 'http://localhost:3400';

export type Data = {
  /**
   * renderDescription (サーバ) の出力。説明が無い・取得に失敗した場合は null で、
   * 表示側はプレーンテキスト表示へフォールバックする (v-html には入らない)。
   */
  descriptionHtml: string | null;
};

async function fetchJson<T>(url: string, cookie: string | undefined): Promise<T | null> {
  const response = await fetch(url, {
    headers: cookie ? { cookie } : undefined,
  });
  if (!response.ok) return null;
  return (await response.json()) as T;
}

/** GET /v1/tenants は配列と { tenants } の両形がある (useResolvedTenantId と同じ吸収)。 */
function toTenantList(data: unknown): TenantResponse[] {
  if (Array.isArray(data)) return data as TenantResponse[];
  const tenants = (data as { tenants?: TenantResponse[] } | null)?.tenants;
  return Array.isArray(tenants) ? tenants : [];
}

export async function data(pageContext: PageContextServer): Promise<Data> {
  const tenantDisplayId = String(pageContext.routeParams?.tenant ?? '');
  const projectKey = String(pageContext.routeParams?.projectKey ?? '');
  const taskId = String(pageContext.routeParams?.taskId ?? '');
  const cookie = pageContext.headers?.cookie;

  if (!tenantDisplayId || !projectKey || !taskId) return { descriptionHtml: null };

  // 取得失敗は throw せず null へ倒す: 説明の KFM 表示は付加価値であり、
  // ここで 500 にするとページ本体 (クライアント側の取得・編集) まで巻き添えになる。
  try {
    const tenants = toTenantList(await fetchJson<unknown>(`${API_BASE}/v1/tenants`, cookie));
    const tenantId = tenants.find((tenant) => tenant.display_id === tenantDisplayId)?.id;
    if (!tenantId) return { descriptionHtml: null };

    const projects = await fetchJson<ProjectResponse[]>(
      `${API_BASE}/v1/tenants/${tenantId}/projects`,
      cookie,
    );
    const projectId = projects?.find((project) => project.key === projectKey)?.id;
    if (!projectId) return { descriptionHtml: null };

    const task = await fetchJson<TaskDetail>(
      `${API_BASE}/v1/tenants/${tenantId}/projects/${projectId}/tasks/${encodeURIComponent(taskId)}`,
      cookie,
    );
    if (!task?.description) return { descriptionHtml: null };

    // scope はタスク UUID で決定的 (同一入力 → 同一 HTML)。URL の seq key (例 "ENG-42")
    // ではなく UUID を使うのは、scope の文字集合制約 [A-Za-z0-9_-]+ を常に満たすため。
    return {
      descriptionHtml: await renderDescription(task.description, { scope: `task-${task.id}` }),
    };
  } catch {
    return { descriptionHtml: null };
  }
}
