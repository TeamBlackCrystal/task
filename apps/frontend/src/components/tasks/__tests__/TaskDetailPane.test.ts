import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { computed, nextTick, ref } from 'vue';
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils';
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query';
import type { components } from '@/generated/api';

type TaskDetail = components['schemas']['TaskDetailResponse'];

const task: TaskDetail = {
  assignees: [],
  created_at: '2026-07-16T00:00:00Z',
  custom_field_values: [],
  id: 'task-id',
  is_archived: false,
  labels: [],
  priority: 'Medium',
  progress_pct: 30,
  project_id: 'project-id',
  seq_id: 42,
  status_id: 'status-id',
  title: 'ペイン用タスク',
  updated_at: '2026-07-16T00:00:00Z',
  // 説明の KFM 描画のテストで差し替える
  description: null as string | null,
};

const confirmDelete = vi.fn();
// useTaskDetail に渡された引数を捕捉し、onAfterDelete などの配線を検証する。
let capturedParams: {
  tenantDisplayId: () => string;
  projectKey: () => string;
  taskId: () => string;
  onAfterDelete?: (listHref: string) => void;
} | null = null;

vi.mock('@/composables/useTaskDetail', () => ({
  useTaskDetail: vi.fn((params) => {
    capturedParams = params;
    return {
      displayTask: computed(() => task),
      statuses: computed(() => []),
      projectLabels: computed(() => []),
      selectedStatusId: ref(task.status_id),
      statusUpdating: computed(() => false),
      statusError: ref(null),
      labelsUpdating: computed(() => false),
      labelsError: ref(null),
      fieldUpdating: computed(() => ({})),
      fieldErrors: ref({}),
      isLoading: computed(() => false),
      isNotFound: computed(() => false),
      isError: computed(() => false),
      onStatusChange: vi.fn(),
      onSaveTitle: vi.fn(),
      onSaveDescription: vi.fn(),
      onSaveProgressPct: vi.fn(),
      onSaveSoftDeadline: vi.fn(),
      onSaveHardDeadline: vi.fn(),
      onSaveLabels: vi.fn(),
      deleteError: ref(null),
      deletePending: computed(() => false),
      confirmDelete,
    };
  }),
}));

import TaskDetailPane from '../TaskDetailPane.vue';

enableAutoUnmount(afterEach);

afterEach(() => {
  vi.unstubAllGlobals();
});

let queryClient: QueryClient;

function mountPane() {
  return mount(TaskDetailPane, {
    props: {
      tenantDisplayId: 'acme',
      projectKey: 'ENG',
      taskId: 'ENG-42',
    },
    global: { plugins: [[VueQueryPlugin, { queryClient }]] },
  });
}

describe('TaskDetailPane', () => {
  beforeEach(() => {
    confirmDelete.mockReset();
    capturedParams = null;
    task.description = null;
    queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
  });

  it('選択タスクのタイトルを描画する', () => {
    const wrapper = mountPane();
    expect(wrapper.text()).toContain('ペイン用タスク');
  });

  it('useTaskDetail に props を getter で渡す', () => {
    mountPane();
    expect(capturedParams).not.toBeNull();
    expect(capturedParams!.tenantDisplayId()).toBe('acme');
    expect(capturedParams!.projectKey()).toBe('ENG');
    expect(capturedParams!.taskId()).toBe('ENG-42');
  });

  it('閉じるタブのクリックで close を emit する', async () => {
    const wrapper = mountPane();
    await wrapper.get('button[aria-label="詳細を閉じる"]').trigger('click');
    expect(wrapper.emitted('close')).toBeTruthy();
  });

  /*
   * 分割ビューは選択がクライアント操作なので、詳細ページのような server data hook
   * （@taskId/+data.ts）が選択中タスクに追従できず、説明が素の markdown のまま
   * 出ていた。KFM をクライアントへ載せない（+417.5 KB）ため、描画はサーバの
   * /internal/render-description に置いて結果だけを取る配線にしてある。
   */
  describe('説明の KFM 描画', () => {
    it('サーバの描画結果を v-html で出す（素の markdown を出さない）', async () => {
      task.description = '# 見出し';
      const fetchMock = vi.fn(
        async (_url: string, _init: RequestInit) =>
          new Response(JSON.stringify({ html: '<h1>見出し</h1>' }), {
            status: 200,
            headers: { 'Content-Type': 'application/json' },
          }),
      );
      vi.stubGlobal('fetch', fetchMock);

      const wrapper = mountPane();
      await flushPromises();

      const rendered = wrapper.get('[data-task-description-html]');
      expect(rendered.html()).toContain('<h1>見出し</h1>');
      // 記法がそのまま見えていたのが直したい症状
      expect(rendered.text()).not.toContain('# 見出し');

      // 描画元はタスク UUID と本文。scope の組み立てはサーバ側に閉じる
      const [url, init] = fetchMock.mock.calls[0];
      expect(url).toBe('/internal/render-description');
      expect(JSON.parse(init.body as string)).toEqual({
        taskId: 'task-id',
        description: '# 見出し',
      });
    });

    it('描画に失敗したらプレーン表示へ倒す（v-html には入れない）', async () => {
      task.description = '# 見出し';
      vi.stubGlobal(
        'fetch',
        vi.fn(async () => new Response('boom', { status: 500 })),
      );

      const wrapper = mountPane();
      await flushPromises();

      expect(wrapper.find('[data-task-description-html]').exists()).toBe(false);
      expect(wrapper.text()).toContain('# 見出し');
    });

    it('説明が無いタスクでは描画を呼ばない', async () => {
      const fetchMock = vi.fn();
      vi.stubGlobal('fetch', fetchMock);

      mountPane();
      await flushPromises();

      expect(fetchMock).not.toHaveBeenCalled();
    });
  });

  it('削除成功（onAfterDelete）でペインを閉じる', async () => {
    const wrapper = mountPane();
    expect(typeof capturedParams!.onAfterDelete).toBe('function');
    capturedParams!.onAfterDelete!('/acme/projects/ENG/tasks');
    await nextTick();
    expect(wrapper.emitted('close')).toBeTruthy();
  });
});
