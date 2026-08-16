import { effectScope, nextTick, ref } from 'vue';
import { describe, expect, it } from 'vitest';

import {
  buildTasksListQueryParams,
  taskListPlaceholderData,
  useTaskLabelFilter,
  watchAvailableTaskLabels,
} from '../task-list-label-filter';

describe('タスク一覧のラベルフィルタ', () => {
  it('選択ラベルをquery.label_idへ反映し、ページを先頭へ戻す', async () => {
    const scope = effectScope();
    const pagination = ref({ pageIndex: 3, pageSize: 20 });
    const projectKey = ref('ENG');
    const selectedLabelId = scope.run(
      () => useTaskLabelFilter(pagination, projectKey).selectedLabelId,
    )!;

    selectedLabelId.value = 'label-bug';
    await nextTick();

    expect(pagination.value.pageIndex).toBe(0);
    expect(
      buildTasksListQueryParams('tenant-1', 'project-1', pagination.value, selectedLabelId.value)
        .params.query.label_id,
    ).toBe('label-bug');
    scope.stop();
  });

  it('同一ラベルのページ送りでは前ページのデータをplaceholderとして維持する', () => {
    const previousData = { tasks: [{ id: 'task-1' }], total: 21 };
    const previousQuery = {
      queryKey: [
        'get',
        '/tasks',
        buildTasksListQueryParams(
          'tenant-1',
          'project-1',
          { pageIndex: 0, pageSize: 20 },
          'label-bug',
        ),
      ],
    };

    expect(taskListPlaceholderData(previousData, previousQuery, 'project-1', 'label-bug')).toBe(
      previousData,
    );
  });

  it('ラベルを切り替えた場合は旧条件のデータをplaceholderに使わない', () => {
    const previousData = { tasks: [{ id: 'task-1' }], total: 1 };
    const previousQuery = {
      queryKey: [
        'get',
        '/tasks',
        buildTasksListQueryParams(
          'tenant-1',
          'project-1',
          { pageIndex: 0, pageSize: 20 },
          'label-bug',
        ),
      ],
    };

    expect(
      taskListPlaceholderData(previousData, previousQuery, 'project-1', 'label-feature'),
    ).toBeUndefined();
  });

  it('選択中のラベルが一覧から消えたら選択を解除して先頭ページへ戻す', async () => {
    const scope = effectScope();
    const pagination = ref({ pageIndex: 0, pageSize: 20 });
    const projectKey = ref('ENG');
    const projectLabels = ref<readonly { id: string }[]>([{ id: 'label-bug' }]);
    const selectedLabelId = scope.run(() => {
      const state = useTaskLabelFilter(pagination, projectKey);
      watchAvailableTaskLabels(state.selectedLabelId, projectLabels);
      return state.selectedLabelId;
    })!;
    selectedLabelId.value = 'label-bug';
    await nextTick();
    pagination.value = { ...pagination.value, pageIndex: 4 };

    projectLabels.value = [{ id: 'label-feature' }];
    await nextTick();
    await nextTick();

    expect(selectedLabelId.value).toBeNull();
    expect(pagination.value.pageIndex).toBe(0);
    scope.stop();
  });

  it('プロジェクトを切り替えたらラベル選択を解除して先頭ページへ戻す', async () => {
    const scope = effectScope();
    const pagination = ref({ pageIndex: 0, pageSize: 20 });
    const projectKey = ref('ENG');
    const selectedLabelId = scope.run(
      () => useTaskLabelFilter(pagination, projectKey).selectedLabelId,
    )!;
    selectedLabelId.value = 'label-bug';
    await nextTick();
    pagination.value = { ...pagination.value, pageIndex: 4 };

    projectKey.value = 'OPS';
    await nextTick();
    await nextTick();

    expect(selectedLabelId.value).toBeNull();
    expect(pagination.value.pageIndex).toBe(0);
    scope.stop();
  });
});
