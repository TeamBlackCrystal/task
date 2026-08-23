// タスク詳細ページの server data hook (https://vike.dev/data)。
// KFM の SSR 契約 (@/lib/markup-renderer/index.ts) の消費側:
// renderDescription はここ (サーバ) で一度だけ実行し、コンポーネントは
// descriptionHtml を v-html で受けるだけにする。クライアント再パースはしない
// (markup-renderer を client へ載せると +417.5 KB — kfm-client-registry テスト参照)。
// +data は既定で server-only のため、この import が client バンドルへ入ることはない。
import type { PageContextServer } from 'vike/types';

import { API_BASE } from '#/api-base';
import type { components } from '@/generated/api';
import { renderDescription } from '@/lib/markup-renderer';

type TenantResponse = components['schemas']['TenantResponse'];
type ProjectResponse = components['schemas']['ProjectResponse'];
type TaskDetail = components['schemas']['TaskDetailResponse'];

export type Data = {
  /**
   * renderDescription (サーバ) の出力。説明が無い・取得に失敗した・本文が
   * MAX_DESCRIPTION_LENGTH を超える場合は null で、
   * 表示側はプレーンテキスト表示へフォールバックする (v-html には入らない)。
   */
  descriptionHtml: string | null;
  /**
   * renderDescription へ渡した入力そのもの。表示側 (TaskDetailHub) は最新の
   * task.description と厳密一致するときだけ descriptionHtml を v-html へ流す
   * （保存直後・reload 失敗・他者更新で古い HTML が出る経路を塞ぐ照合キー）。
   * ハッシュではなく原文を載せるのは照合を同期・厳密 (衝突なし) にするため。
   * payload に説明文が二重に載る分は、説明がタスク単位の短文であることから許容する。
   */
  descriptionSource: string | null;
};

const EMPTY: Data = { descriptionHtml: null, descriptionSource: null };

// 本文長の上限: 超過分は KFM 描画せず EMPTY (プレーンテキスト表示) へ倒す。
// renderDescription の CPU も L1 キャッシュ (full-text キー) のメモリも本文長に
// 比例するため、SSR に非有界の入力を入れない。値は GitHub issue 本文の上限
// 65536 文字に合わせる (KFM Phase 1 = github profile の複製レンダラ)。
const MAX_DESCRIPTION_LENGTH = 65_536;

// 遅い backend の取得を有界にする: 3 連続 GET で共有する 1 本の予算。
// 各 GET に個別 timeout を付けると直列で合算 (~9s) になるため、
// data() 開始時点から SSR_FETCH_BUDGET_MS の単一 AbortSignal を全 fetch へ渡す。
const SSR_FETCH_BUDGET_MS = 3000;

async function fetchJson<T>(
  url: string,
  cookie: string | undefined,
  signal: AbortSignal,
): Promise<T | null> {
  const response = await fetch(url, {
    headers: cookie ? { cookie } : undefined,
    signal,
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

  if (!tenantDisplayId || !projectKey || !taskId) return EMPTY;

  // 取得失敗・タイムアウトは throw せず null へ倒す: 説明の KFM 表示は付加価値であり、
  // ここで 500 にするとページ本体 (クライアント側の取得・編集) まで巻き添えになる。
  //
  // 3 連続 GET が要るのは、URL が表示用 id (tenant display_id / project key / seq key)
  // しか持たない一方で backend の各エンドポイントが UUID 階層でしか引けないため:
  // display_id → tenant UUID → project UUID → task の順にしか解決できない。
  // SSR_FETCH_BUDGET_MS の単一 signal を全 GET で共有するので、backend の取得待ちは
  // 最悪 SSR_FETCH_BUDGET_MS で打ち切られ、タイムアウト後はプレーン表示へ倒れる
  // (KFM 表示は次の再読み込みで回復する)。
  // 短縮するには backend に表示用 id で直接引ける口が要る (別途検討)。
  const fetchBudget = AbortSignal.timeout(SSR_FETCH_BUDGET_MS);
  try {
    const tenants = toTenantList(
      await fetchJson<unknown>(`${API_BASE}/v1/tenants`, cookie, fetchBudget),
    );
    const tenantId = tenants.find((tenant) => tenant.display_id === tenantDisplayId)?.id;
    if (!tenantId) return EMPTY;

    const projects = await fetchJson<ProjectResponse[]>(
      `${API_BASE}/v1/tenants/${tenantId}/projects`,
      cookie,
      fetchBudget,
    );
    const projectId = projects?.find((project) => project.key === projectKey)?.id;
    if (!projectId) return EMPTY;

    const task = await fetchJson<TaskDetail>(
      `${API_BASE}/v1/tenants/${tenantId}/projects/${projectId}/tasks/${encodeURIComponent(taskId)}`,
      cookie,
      fetchBudget,
    );
    if (!task?.description) return EMPTY;
    if (task.description.length > MAX_DESCRIPTION_LENGTH) return EMPTY;

    // scope はタスク UUID で決定的 (同一入力 → 同一 HTML)。URL の seq key (例 "ENG-42")
    // ではなく UUID を使うのは、scope の文字集合制約 [A-Za-z0-9_-]+ を常に満たすため。
    return {
      descriptionHtml: await renderDescription(task.description, { scope: `task-${task.id}` }),
      descriptionSource: task.description,
    };
  } catch (error) {
    console.error('[task-description-data] SSR data failed', error);
    return EMPTY;
  }
}
