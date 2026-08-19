import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { computed, nextTick, ref } from 'vue';
import { enableAutoUnmount, mount } from '@vue/test-utils';
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

function mountPane() {
  return mount(TaskDetailPane, {
    props: {
      tenantDisplayId: 'acme',
      projectKey: 'ENG',
      taskId: 'ENG-42',
    },
  });
}

describe('TaskDetailPane', () => {
  beforeEach(() => {
    confirmDelete.mockReset();
    capturedParams = null;
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

  it('削除成功（onAfterDelete）でペインを閉じる', async () => {
    const wrapper = mountPane();
    expect(typeof capturedParams!.onAfterDelete).toBe('function');
    capturedParams!.onAfterDelete!('/acme/projects/ENG/tasks');
    await nextTick();
    expect(wrapper.emitted('close')).toBeTruthy();
  });
});
