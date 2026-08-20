import { readFileSync } from 'node:fs';
import path from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';
import { enableAutoUnmount, mount } from '@vue/test-utils';
import type { components } from '@/generated/api';
import TaskDetailHub from '../TaskDetailHub.vue';

enableAutoUnmount(afterEach);

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
  seq_id: 1,
  status_id: 'status-id',
  title: 'Test task',
  updated_at: '2026-07-16T00:00:00Z',
};

const bugLabel: components['schemas']['LabelResponse'] = {
  id: 'label-bug',
  name: 'bug',
  description: '',
  color: '#e11d48',
  icon_url: null,
  project_id: 'project-id',
};

const featureLabel: components['schemas']['LabelResponse'] = {
  id: 'label-feature',
  name: 'feature',
  description: '',
  color: '#3b82f6',
  icon_url: null,
  project_id: 'project-id',
};

describe('TaskDetailHub', () => {
  it('emits the selected soft deadline through the SET path', async () => {
    const wrapper = mount(TaskDetailHub, {
      props: {
        task,
        projectKey: 'TEST',
        statuses: [],
        statusId: task.status_id,
      },
    });

    await wrapper.get('button[aria-label="ソフト期限を編集"]').trigger('click');
    const deadlineInput = wrapper.get('input[aria-label="ソフト期限"]');
    await deadlineInput.setValue('2026-08-23');
    await deadlineInput.trigger('blur');

    expect(wrapper.emitted('save:soft_deadline')).toEqual([['2026-08-23']]);
  });

  it('does not save an empty progress value as zero percent', async () => {
    const wrapper = mount(TaskDetailHub, {
      props: {
        task,
        projectKey: 'TEST',
        statuses: [],
        statusId: task.status_id,
      },
    });

    const progressButton = wrapper.findAll('button').find((button) => button.text() === '30%');
    expect(progressButton).toBeDefined();
    await progressButton!.trigger('click');
    const progressInput = wrapper.get('input[aria-label="進捗率"]');
    await progressInput.setValue('');
    await progressInput.trigger('blur');

    expect(wrapper.emitted('save:progress_pct')).toBeUndefined();
    expect(wrapper.find('input[aria-label="進捗率"]').exists()).toBe(false);
    expect(wrapper.text()).toContain('30%');
  });

  it('emits delete-request when the kebab menu delete item is selected', async () => {
    const wrapper = mount(TaskDetailHub, {
      props: {
        task,
        projectKey: 'TEST',
        statuses: [],
        statusId: task.status_id,
      },
      attachTo: document.body,
    });

    await wrapper.get('button[aria-label="タスク操作"]').trigger('click');
    const deleteItem = document.body.querySelector('[data-slot="dropdown-menu-item"]');
    expect(deleteItem?.textContent).toContain('削除');
    deleteItem?.dispatchEvent(new MouseEvent('click', { bubbles: true }));

    expect(wrapper.emitted('delete-request')).toHaveLength(1);
    wrapper.unmount();
  });

  it('タスクのラベルをチップ表示する', () => {
    const wrapper = mount(TaskDetailHub, {
      props: {
        task: { ...task, labels: [bugLabel] },
        projectKey: 'TEST',
        statuses: [],
        statusId: task.status_id,
      },
    });

    const section = wrapper.get('[data-task-labels]');
    expect(section.text()).toContain('bug');
    expect(section.text()).not.toContain('ラベルなし');
  });

  it('未付与ラベルのチェックで既存 ID に追加した save:label_ids を emit する', async () => {
    const wrapper = mount(TaskDetailHub, {
      props: {
        task: { ...task, labels: [bugLabel] },
        projectKey: 'TEST',
        statuses: [],
        statusId: task.status_id,
        projectLabels: [bugLabel, featureLabel],
      },
      attachTo: document.body,
    });

    await wrapper.get('button[aria-label="ラベルを編集"]').trigger('click');
    const items = document.body.querySelectorAll('[data-slot="dropdown-menu-checkbox-item"]');
    const featureItem = Array.from(items).find((el) => el.textContent?.includes('feature'));
    expect(featureItem).toBeDefined();
    featureItem?.dispatchEvent(new MouseEvent('click', { bubbles: true }));

    expect(wrapper.emitted('save:label_ids')).toEqual([[['label-bug', 'label-feature']]]);
    wrapper.unmount();
  });

  it('ラベル一覧の取得失敗時は「ラベルがありません」ではなくエラーを表示する', async () => {
    const wrapper = mount(TaskDetailHub, {
      props: {
        task,
        projectKey: 'TEST',
        statuses: [],
        statusId: task.status_id,
        projectLabelsError: true,
      },
      attachTo: document.body,
    });

    await wrapper.get('button[aria-label="ラベルを編集"]').trigger('click');
    expect(document.body.textContent).toContain('ラベルを読み込めませんでした');
    expect(document.body.textContent).not.toContain('ラベルがありません');
    wrapper.unmount();
  });

  it('取得失敗時にキャッシュ済みラベルが残っていてもチェックボックスを無効化する', async () => {
    const wrapper = mount(TaskDetailHub, {
      props: {
        task,
        projectKey: 'TEST',
        statuses: [],
        statusId: task.status_id,
        projectLabels: [bugLabel],
        projectLabelsError: true,
      },
      attachTo: document.body,
    });

    await wrapper.get('button[aria-label="ラベルを編集"]').trigger('click');
    const items = document.body.querySelectorAll('[data-slot="dropdown-menu-checkbox-item"]');
    expect(items.length).toBe(1);
    expect(items[0].getAttribute('data-disabled')).not.toBeNull();
    wrapper.unmount();
  });

  it('ラベル一覧の取得中は編集ボタンを無効化する', () => {
    const wrapper = mount(TaskDetailHub, {
      props: {
        task,
        projectKey: 'TEST',
        statuses: [],
        statusId: task.status_id,
        projectLabelsLoading: true,
      },
    });

    expect(wrapper.get('button[aria-label="ラベルを編集"]').attributes('disabled')).toBeDefined();
  });

  it('付与済みラベルのチェック解除で除外した save:label_ids を emit する', async () => {
    const wrapper = mount(TaskDetailHub, {
      props: {
        task: { ...task, labels: [bugLabel] },
        projectKey: 'TEST',
        statuses: [],
        statusId: task.status_id,
        projectLabels: [bugLabel, featureLabel],
      },
      attachTo: document.body,
    });

    await wrapper.get('button[aria-label="ラベルを編集"]').trigger('click');
    const items = document.body.querySelectorAll('[data-slot="dropdown-menu-checkbox-item"]');
    const bugItem = Array.from(items).find((el) => el.textContent?.includes('bug'));
    expect(bugItem).toBeDefined();
    bugItem?.dispatchEvent(new MouseEvent('click', { bubbles: true }));

    expect(wrapper.emitted('save:label_ids')).toEqual([[[]]]);
    wrapper.unmount();
  });

  it('プロジェクト一覧に無いラベルも送信集合に保持する（古い一覧キャッシュで暗黙解除しない）', async () => {
    const staleLabel: components['schemas']['LabelResponse'] = {
      ...bugLabel,
      id: 'label-stale',
      name: 'stale',
    };
    const wrapper = mount(TaskDetailHub, {
      props: {
        task: { ...task, labels: [bugLabel, staleLabel] },
        projectKey: 'TEST',
        statuses: [],
        statusId: task.status_id,
        projectLabels: [bugLabel, featureLabel],
      },
      attachTo: document.body,
    });

    await wrapper.get('button[aria-label="ラベルを編集"]').trigger('click');
    const items = document.body.querySelectorAll('[data-slot="dropdown-menu-checkbox-item"]');
    const featureItem = Array.from(items).find((el) => el.textContent?.includes('feature'));
    expect(featureItem).toBeDefined();
    featureItem?.dispatchEvent(new MouseEvent('click', { bubbles: true }));

    // label-stale が一覧に無いのはキャッシュが古いだけかもしれないので落とさない
    expect(wrapper.emitted('save:label_ids')).toEqual([
      [['label-bug', 'label-stale', 'label-feature']],
    ]);
    wrapper.unmount();
  });
});

describe('TaskDetailHub description KFM 表示', () => {
  const RENDERED_HTML = '<h1 id="user-content-task-uuid-h">見出し</h1><p><strong>強調</strong></p>';

  it('descriptionHtml があれば kfm-content の器へ v-html でそのまま入る', () => {
    const wrapper = mount(TaskDetailHub, {
      props: {
        task: { ...task, description: '# 見出し\n\n**強調**' },
        descriptionHtml: RENDERED_HTML,
        projectKey: 'TEST',
        statuses: [],
        statusId: task.status_id,
      },
    });

    const container = wrapper.get('[data-task-description-html]');
    expect(container.classes()).toContain('kfm-content');
    expect(container.element.innerHTML).toBe(RENDERED_HTML);
    // 生テキスト表示 (プレーン分岐) は出ない
    expect(wrapper.find('p.whitespace-pre-wrap').exists()).toBe(false);
  });

  it('descriptionHtml が無ければ生テキストのプレーン表示のまま (クライアント再パースなし)', () => {
    const wrapper = mount(TaskDetailHub, {
      props: {
        task: { ...task, description: '**強調** <em>raw</em>' },
        projectKey: 'TEST',
        statuses: [],
        statusId: task.status_id,
      },
    });

    // Markdown 記法が文字のまま見える = クライアントでは一切パースしていない
    expect(wrapper.get('p.whitespace-pre-wrap').text()).toContain('**強調**');
    expect(wrapper.find('[data-task-description-html]').exists()).toBe(false);
    expect(wrapper.find('strong').exists()).toBe(false);
    // 生テキスト中のタグ断片も要素化されない (v-html へ流れていない)
    expect(wrapper.find('em').exists()).toBe(false);
  });

  it('KFM 表示中は鉛筆ボタンから編集へ入り、textarea には生テキストが入る', async () => {
    const wrapper = mount(TaskDetailHub, {
      props: {
        task: { ...task, description: '# 見出し' },
        descriptionHtml: RENDERED_HTML,
        projectKey: 'TEST',
        statuses: [],
        statusId: task.status_id,
      },
    });

    await wrapper.get('button[aria-label="説明を編集"]').trigger('click');
    const textarea = wrapper.get('textarea[aria-label="説明"]');
    expect((textarea.element as HTMLTextAreaElement).value).toBe('# 見出し');
    // 編集中は KFM 表示は消える
    expect(wrapper.find('[data-task-description-html]').exists()).toBe(false);
  });

  it('説明が空なら descriptionHtml が残っていても stale HTML を出さない', () => {
    const wrapper = mount(TaskDetailHub, {
      props: {
        task: { ...task, description: null },
        descriptionHtml: RENDERED_HTML,
        projectKey: 'TEST',
        statuses: [],
        statusId: task.status_id,
      },
    });

    expect(wrapper.find('[data-task-description-html]').exists()).toBe(false);
    expect(wrapper.text()).toContain('説明はありません');
  });
});

describe('TaskDetailHub v-html 経路の source 契約', () => {
  it('SFC 内の v-html は descriptionHtml へのバインド 1 箇所だけ', () => {
    const source = readFileSync(path.join(__dirname, '../TaskDetailHub.vue'), 'utf-8');
    const bindings = source.match(/v-html="[^"]*"/g) ?? [];
    expect(bindings).toEqual(['v-html="descriptionHtml"']);
  });
});
