import { describe, it, expect, vi, beforeEach } from 'vitest';
import { defineComponent } from 'vue';
import { mount, flushPromises } from '@vue/test-utils';
import { VueQueryPlugin, QueryClient } from '@tanstack/vue-query';
import type { paths } from '@/generated/api';
import { useTaskActivities, ACTIVITIES_PAGE_SIZE } from '../useTaskActivities';

// vi.mock の factory から参照するため hoisted に置く
const { control, requestLog, fetchMock } = vi.hoisted(() => {
  function jsonResponse(body: unknown, status = 200) {
    return new Response(JSON.stringify(body), {
      status,
      headers: { 'Content-Type': 'application/json' },
    });
  }

  const control: { total: number; fail: boolean } = { total: 0, fail: false };
  const requestLog: { limit: number; offset: number }[] = [];

  const fetchMock = async (input: Request) => {
    if (control.fail) return jsonResponse({ message: 'boom' }, 500);
    const url = new URL(input.url);
    const limit = Number(url.searchParams.get('limit'));
    const offset = Number(url.searchParams.get('offset'));
    requestLog.push({ limit, offset });

    const activities = Array.from({
      length: Math.max(0, Math.min(limit, control.total - offset)),
    }).map((_, i) => ({
      id: `act-${offset + i}`,
      event_type: 'status_changed',
      payload: {},
      created_at: '2026-06-01T00:00:00Z',
      user: null,
    }));
    return jsonResponse({ activities, total: control.total });
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
    control.total = 0;
    control.fail = false;
    requestLog.length = 0;
  });

  // 全件取ると、長く使われたタスクほど DB・レスポンス・描画のコストが上限なく伸びる
  it('開いた時点では先頭の 1 ページだけ取る', async () => {
    control.total = 137;
    mountHost();
    await flushPromises();

    expect(requestLog).toEqual([{ limit: ACTIVITIES_PAGE_SIZE, offset: 0 }]);
    expect(activities.activities.value).toHaveLength(ACTIVITIES_PAGE_SIZE);
    expect(activities.hasMoreActivities.value).toBe(true);
  });

  it('もっと見るで続きを足す（重複しない）', async () => {
    control.total = 137;
    mountHost();
    await flushPromises();

    activities.loadMoreActivities();
    await flushPromises();

    expect(requestLog).toEqual([
      { limit: ACTIVITIES_PAGE_SIZE, offset: 0 },
      { limit: ACTIVITIES_PAGE_SIZE, offset: ACTIVITIES_PAGE_SIZE },
    ]);
    const ids = activities.activities.value.map((item) => item.id);
    expect(ids).toHaveLength(ACTIVITIES_PAGE_SIZE * 2);
    expect(new Set(ids).size).toBe(ids.length);
    expect(activities.hasMoreActivities.value).toBe(true);
  });

  it('取り切ったら導線を出さない', async () => {
    // 1 ページに収まらないが 2 ページ目で終わる件数
    control.total = ACTIVITIES_PAGE_SIZE + 3;
    mountHost();
    await flushPromises();
    expect(activities.hasMoreActivities.value).toBe(true);

    activities.loadMoreActivities();
    await flushPromises();

    expect(activities.activities.value).toHaveLength(control.total);
    expect(activities.hasMoreActivities.value).toBe(false);
  });

  it('1 ページに収まるなら導線を出さない', async () => {
    control.total = 7;
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
