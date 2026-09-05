import { describe, it, expect, vi, beforeEach } from 'vitest';
import { defineComponent } from 'vue';
import { mount, flushPromises } from '@vue/test-utils';
import { VueQueryPlugin, QueryClient } from '@tanstack/vue-query';
import type { paths } from '@/generated/api';
import { useTaskActivities, ACTIVITIES_PAGE_SIZE } from '../useTaskActivities';

/** 新しいほど大きい ID。並びは ID の降順 = 新しい順 */
function actId(n: number) {
  return `act-${String(n).padStart(4, '0')}`;
}

// vi.mock の factory から参照するため hoisted に置く
const { control, requestLog, fetchMock } = vi.hoisted(() => {
  function jsonResponse(body: unknown, status = 200) {
    return new Response(JSON.stringify(body), {
      status,
      headers: { 'Content-Type': 'application/json' },
    });
  }

  const control: { ids: string[]; fail: boolean } = { ids: [], fail: false };
  const requestLog: { limit: number; cursor: string | null }[] = [];

  // backend の keyset と同じ形にする。カーソルは「並びの中の位置」ではなく
  // 並び順のキーそのものなので、取得のあいだに行が増減しても境界がずれない
  const fetchMock = async (input: Request) => {
    if (control.fail) return jsonResponse({ message: 'boom' }, 500);
    const url = new URL(input.url);
    const limit = Number(url.searchParams.get('limit'));
    const cursor = url.searchParams.get('cursor');
    requestLog.push({ limit, cursor });

    const ordered = [...control.ids].sort().reverse();
    const after = cursor ? ordered.filter((id) => id < cursor) : ordered;
    const page = after.slice(0, limit);
    return jsonResponse({
      activities: page.map((id) => ({
        id,
        event_type: 'status_changed',
        payload: {},
        created_at: '2026-06-01T00:00:00Z',
        user: null,
      })),
      total: control.ids.length,
      next_cursor: after.length > limit ? page[page.length - 1] : null,
    });
  };

  return { control, requestLog, fetchMock };
});

vi.mock('@/lib/api-vue-query', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api-vue-query')>();
  const { default: createFetchClient } = await import('openapi-fetch');
  const { createClient } = await import('@koyori-app/openapi-vue-query');
  const testFetchClient = createFetchClient<paths>({
    baseUrl: 'http://test.local/api',
    fetch: (req: Request) => fetchMock(req),
  });
  return {
    ...actual,
    fetchClient: testFetchClient,
    apiClient: createClient<paths>(testFetchClient),
  };
});

describe('useTaskActivities', () => {
  let queryClient: QueryClient;
  let activities: ReturnType<typeof useTaskActivities>;

  /** 0..count-1 の履歴を積んだ状態にする */
  function seed(count: number) {
    control.ids = Array.from({ length: count }, (_, i) => actId(i));
  }

  function mountHost() {
    const Host = defineComponent({
      setup() {
        activities = useTaskActivities({
          tenantId: 'tenant-1',
          projectId: 'project-1',
          taskId: 'ENG-1',
        });
        return () => null;
      },
    });
    return mount(Host, { global: { plugins: [[VueQueryPlugin, { queryClient }]] } });
  }

  beforeEach(() => {
    queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    control.ids = [];
    control.fail = false;
    requestLog.length = 0;
  });

  // 全件取ると、長く使われたタスクほど DB・レスポンス・描画のコストが上限なく伸びる
  it('開いた時点では先頭の 1 ページだけ取る', async () => {
    seed(137);
    mountHost();
    await flushPromises();

    expect(requestLog).toEqual([{ limit: ACTIVITIES_PAGE_SIZE, cursor: null }]);
    expect(activities.activities.value).toHaveLength(ACTIVITIES_PAGE_SIZE);
    expect(activities.hasMoreActivities.value).toBe(true);
  });

  it('もっと見るで続きを足す（重複しない）', async () => {
    seed(137);
    mountHost();
    await flushPromises();

    activities.loadMoreActivities();
    await flushPromises();

    // 2 回目は 1 ページ目の最後の ID を鍵にして続きを引く
    expect(requestLog).toEqual([
      { limit: ACTIVITIES_PAGE_SIZE, cursor: null },
      { limit: ACTIVITIES_PAGE_SIZE, cursor: actId(137 - ACTIVITIES_PAGE_SIZE) },
    ]);
    const ids = activities.activities.value.map((item) => item.id);
    expect(ids).toHaveLength(ACTIVITIES_PAGE_SIZE * 2);
    expect(new Set(ids).size).toBe(ids.length);
    expect(activities.hasMoreActivities.value).toBe(true);
  });

  // 欠落・重複の再発ガード。offset で継いでいたときは、1 ページ目を読んだ後に
  // 履歴が 1 件積まれるだけで 2 ページ目の境界が 1 件ぶんずれ、境界の行が二重に出ていた
  it('読んでいる最中に履歴が積まれても、重複も欠落もしない', async () => {
    seed(30);
    mountHost();
    await flushPromises();
    const firstPage = activities.activities.value.map((item) => item.id);

    // 別の操作で新しい履歴が 3 件積まれる（並びの先頭側に入る）
    control.ids.push(actId(100), actId(101), actId(102));

    activities.loadMoreActivities();
    await flushPromises();

    const ids = activities.activities.value.map((item) => item.id);
    expect(new Set(ids).size).toBe(ids.length);
    // 1 ページ目より古い 10 件が、1 件も飛ばずに続きとして出る
    expect(ids).toEqual([...firstPage, ...Array.from({ length: 10 }, (_, i) => actId(9 - i))]);
    expect(activities.hasMoreActivities.value).toBe(false);
  });

  it('取り切ったら導線を出さない', async () => {
    // 1 ページに収まらないが 2 ページ目で終わる件数
    seed(ACTIVITIES_PAGE_SIZE + 3);
    mountHost();
    await flushPromises();
    expect(activities.hasMoreActivities.value).toBe(true);

    activities.loadMoreActivities();
    await flushPromises();

    expect(activities.activities.value).toHaveLength(ACTIVITIES_PAGE_SIZE + 3);
    expect(activities.hasMoreActivities.value).toBe(false);
  });

  it('1 ページに収まるなら導線を出さない', async () => {
    seed(7);
    mountHost();
    await flushPromises();

    expect(activities.activities.value).toHaveLength(7);
    expect(activities.hasMoreActivities.value).toBe(false);
  });

  it('取得の失敗は欄の中で倒す', async () => {
    control.fail = true;
    mountHost();
    await flushPromises();

    expect(activities.activitiesError.value).toBe(true);
    expect(activities.activities.value).toEqual([]);
  });
});
