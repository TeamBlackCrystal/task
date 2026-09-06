import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { ref, nextTick } from 'vue';
import { mount, flushPromises, DOMWrapper } from '@vue/test-utils';
import { VueQueryPlugin, QueryClient } from '@tanstack/vue-query';

const isHydrated = ref(true);
const isPending = ref(false);
const mutateAsync = vi.fn();

vi.mock('@/composables/useHydrated', () => ({
  useHydrated: () => isHydrated,
}));

vi.mock('@/lib/task-display', () => ({
  taskDetailHref: vi.fn(() => '/tenant/projects/proj/tasks/1'),
  PRIORITY_CONFIG: {
    CriticalFire: { label: '緊急', color: '#dc2626', icon: {} },
    Critical: { label: '重大', color: '#ef4444', icon: {} },
    High: { label: '高', color: '#f97316', icon: {} },
    Medium: { label: '中', color: '#eab308', icon: {} },
    Low: { label: '低', color: '#6b7280', icon: {} },
    Trivial: { label: '些細', color: '#9ca3af', icon: {} },
  },
}));

vi.mock('@/lib/api-vue-query', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api-vue-query')>();
  return {
    ...actual,
    apiClient: {
      ...actual.apiClient,
      useMutation: vi.fn(() => ({
        isPending,
        mutateAsync,
      })),
    },
  };
});

/*
 * MarkdownEditor (CodeMirror) は happy-dom では起動できないため textarea の
 * 代役に差し替える (説明は stub 側)。実物は story が実ブラウザで見る。
 */
vi.mock('@/components/markdown/MarkdownEditor.vue', async () => ({
  default: (await import('@/components/markdown/__tests__/markdown-editor-stub')).default,
}));

import CreateTaskDialog from '../CreateTaskDialog.vue';

const statuses = [
  {
    id: 'status-1',
    name: 'Todo',
    color: '#2563eb',
    is_default: true,
    is_done_state: false,
    created_at: '2026-01-01T00:00:00.000Z',
    position: 0,
    project_id: 'project-uuid',
  },
];

const createdTask = {
  id: 'task-1',
  seq_id: 1,
  title: 'New task',
  status_id: 'status-1',
  priority: 'Medium',
};

const labels = [
  {
    id: 'label-bug',
    name: 'bug',
    description: '',
    color: '#e11d48',
    icon_url: null,
    project_id: 'project-uuid',
  },
  {
    id: 'label-feature',
    name: 'feature',
    description: '',
    color: '#3b82f6',
    icon_url: null,
    project_id: 'project-uuid',
  },
];

type MountOptions = {
  open?: boolean;
  labels?: typeof labels;
  labelsLoading?: boolean;
  labelsError?: boolean;
};

function mountDialog(queryClient: QueryClient, options: MountOptions = {}) {
  return mount(CreateTaskDialog, {
    props: {
      open: options.open ?? true,
      tenantId: 'tenant-uuid',
      projectId: 'project-uuid',
      projectKey: 'PROJ',
      statuses,
      labels: options.labels,
      labelsLoading: options.labelsLoading,
      labelsError: options.labelsError,
    },
    global: {
      plugins: [[VueQueryPlugin, { queryClient }]],
    },
    attachTo: document.body,
  });
}

function getTitleInput() {
  const input = document.body.querySelector('input[name="title"]');
  if (!input) throw new Error('title input not found');
  return input as HTMLInputElement;
}

function getForm() {
  const form = document.body.querySelector('form');
  if (!form) throw new Error('form not found');
  return form;
}

function getDialog() {
  const dialog = document.body.querySelector('[role="dialog"]');
  if (!dialog) throw new Error('dialog not found');
  return dialog as HTMLElement;
}

describe('CreateTaskDialog double-submit guard', () => {
  let queryClient: QueryClient;
  let resolveMutation: ((value: typeof createdTask) => void) | undefined;

  beforeEach(() => {
    isHydrated.value = true;
    isPending.value = false;
    mutateAsync.mockReset();
    resolveMutation = undefined;
    mutateAsync.mockImplementation(
      () =>
        new Promise<typeof createdTask>((resolve) => {
          isPending.value = true;
          resolveMutation = resolve;
        }),
    );
    queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false },
        mutations: { retry: false },
      },
    });
    vi.spyOn(queryClient, 'invalidateQueries').mockResolvedValue(undefined as never);
  });

  afterEach(() => {
    vi.restoreAllMocks();
    document.body.innerHTML = '';
  });

  it('does not fire a second mutation while create is pending (submit + Enter)', async () => {
    const wrapper = mountDialog(queryClient);
    await nextTick();

    const titleInput = new DOMWrapper(getTitleInput());
    await titleInput.setValue('New task');
    const form = new DOMWrapper(getForm());

    await form.trigger('submit');
    await nextTick();

    expect(mutateAsync).toHaveBeenCalledTimes(1);
    expect(isPending.value).toBe(true);

    await form.trigger('submit');

    const enter = new KeyboardEvent('keydown', {
      key: 'Enter',
      bubbles: true,
      cancelable: true,
    });
    form.element.dispatchEvent(enter);
    await form.trigger('submit');
    await nextTick();

    expect(mutateAsync).toHaveBeenCalledTimes(1);

    resolveMutation?.(createdTask);
    await flushPromises();

    wrapper.unmount();
  });
});

describe('CreateTaskDialog a11y and cache invalidation', () => {
  let queryClient: QueryClient;
  let invalidateSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    isHydrated.value = true;
    isPending.value = false;
    mutateAsync.mockReset();
    mutateAsync.mockResolvedValue(createdTask);
    queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false },
        mutations: { retry: false },
      },
    });
    invalidateSpy = vi
      .spyOn(queryClient, 'invalidateQueries')
      .mockResolvedValue(undefined as never);
  });

  afterEach(() => {
    vi.restoreAllMocks();
    document.body.innerHTML = '';
  });

  it('closes on Escape from a focused field', async () => {
    const wrapper = mountDialog(queryClient);
    await nextTick();
    const titleInput = getTitleInput();
    titleInput.focus();

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }),
    );
    await nextTick();

    expect(wrapper.emitted('update:open')?.at(-1)).toEqual([false]);
    wrapper.unmount();
  });

  it('keeps Tab focus within the dialog', async () => {
    const wrapper = mountDialog(queryClient);
    await nextTick();
    const dialog = getDialog();
    const focusables = dialog.querySelectorAll<HTMLElement>(
      'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled])',
    );
    expect(focusables.length).toBeGreaterThan(1);

    focusables[0]?.focus();
    for (let i = 0; i < focusables.length + 2; i += 1) {
      document.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true }),
      );
      await nextTick();
      expect(dialog.contains(document.activeElement)).toBe(true);
    }

    wrapper.unmount();
  });

  it('always invalidates the task list after successful create', async () => {
    const wrapper = mountDialog(queryClient);
    await nextTick();
    const titleInput = new DOMWrapper(getTitleInput());
    await titleInput.setValue('New task');
    await new DOMWrapper(getForm()).trigger('submit');

    await vi.waitFor(() => {
      expect(invalidateSpy).toHaveBeenCalledWith({
        queryKey: ['get', '/v1/tenants/{tenant_id}/projects/{project_id}/tasks'],
        refetchType: 'none',
      });
    });
    expect(wrapper.emitted('created')?.[0]).toEqual([createdTask]);
    wrapper.unmount();
  });

  /**
   * 作成に成功すると親が created を受けて open を false にする。その経路は
   * onOpenChange を通らないので、成功表示を閉じるときに捨てないと次に開いた
   * 瞬間に前回の「タスクを作成しました」が出る。
   */
  it('閉じて開き直したとき、前回の成功表示が残らない', async () => {
    const wrapper = mountDialog(queryClient);
    await nextTick();

    await new DOMWrapper(getTitleInput()).setValue('New task');
    await new DOMWrapper(getForm()).trigger('submit');
    await flushPromises();

    expect(document.body.textContent).toContain('タスクを作成しました');

    // 親が閉じる（created を受けた側の挙動）
    await wrapper.setProps({ open: false });
    await nextTick();
    await wrapper.setProps({ open: true });
    await nextTick();

    expect(document.body.textContent).not.toContain('タスクを作成しました');
  });

  it('閉じて開き直したとき、前回の入力が残らない', async () => {
    const wrapper = mountDialog(queryClient);
    await nextTick();

    await new DOMWrapper(getTitleInput()).setValue('書きかけ');
    expect(getTitleInput().value).toBe('書きかけ');

    await wrapper.setProps({ open: false });
    await nextTick();
    await wrapper.setProps({ open: true });
    await nextTick();

    expect(getTitleInput().value).toBe('');
  });

  it('resets form when the dialog closes', async () => {
    const wrapper = mountDialog(queryClient);
    await nextTick();
    await new DOMWrapper(getTitleInput()).setValue('Draft title');

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }),
    );
    await nextTick();

    expect(getTitleInput().value).toBe('');
    wrapper.unmount();
  });
});

describe('CreateTaskDialog pre-hydration form values', () => {
  it('keeps selected status and priority in native FormData before hydration', async () => {
    isHydrated.value = false;
    isPending.value = false;
    mutateAsync.mockReset();
    const queryClient = new QueryClient();

    const wrapper = mountDialog(queryClient);
    await nextTick();

    const form = getForm();
    const formData = new FormData(form);

    expect(form.getAttribute('onsubmit')).toBe('return false;');
    expect(formData.get('status_id')).toBe('status-1');
    expect(formData.get('priority')).toBe('Medium');

    wrapper.unmount();
    isHydrated.value = true;
  });
});

describe('CreateTaskDialog label selection', () => {
  let queryClient: QueryClient;

  beforeEach(() => {
    isHydrated.value = true;
    isPending.value = false;
    mutateAsync.mockReset();
    mutateAsync.mockResolvedValue(createdTask);
    queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false },
        mutations: { retry: false },
      },
    });
    vi.spyOn(queryClient, 'invalidateQueries').mockResolvedValue(undefined as never);
  });

  afterEach(() => {
    document.body.innerHTML = '';
  });

  function labelButton(name: string) {
    const button = Array.from(document.body.querySelectorAll('button[aria-pressed]')).find(
      (el) => el.textContent?.trim() === name,
    );
    if (!button) throw new Error(`label button ${name} not found`);
    return button as HTMLButtonElement;
  }

  it('選択したラベルの label_ids を作成リクエストに含める', async () => {
    const wrapper = mountDialog(queryClient, { labels });
    await nextTick();
    await new DOMWrapper(getTitleInput()).setValue('ラベル付きで作成');

    labelButton('feature').click();
    await nextTick();
    expect(labelButton('feature').getAttribute('aria-pressed')).toBe('true');

    getForm().dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await flushPromises();

    expect(mutateAsync).toHaveBeenCalledTimes(1);
    expect(mutateAsync.mock.calls[0][0].body.label_ids).toEqual(['label-feature']);
    wrapper.unmount();
  });

  it('未選択なら label_ids をリクエストに含めない', async () => {
    const wrapper = mountDialog(queryClient, { labels });
    await nextTick();
    await new DOMWrapper(getTitleInput()).setValue('ラベルなしで作成');

    getForm().dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await flushPromises();

    expect(mutateAsync).toHaveBeenCalledTimes(1);
    expect(mutateAsync.mock.calls[0][0].body.label_ids).toBeUndefined();
    wrapper.unmount();
  });

  it('トグル解除でラベルの選択が外れる', async () => {
    const wrapper = mountDialog(queryClient, { labels });
    await nextTick();
    await new DOMWrapper(getTitleInput()).setValue('トグル確認');

    labelButton('bug').click();
    await nextTick();
    labelButton('bug').click();
    await nextTick();
    expect(labelButton('bug').getAttribute('aria-pressed')).toBe('false');

    getForm().dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await flushPromises();

    expect(mutateAsync.mock.calls[0][0].body.label_ids).toBeUndefined();
    wrapper.unmount();
  });

  it('projectId が変わったら旧プロジェクトのラベル選択を持ち越さない', async () => {
    const wrapper = mountDialog(queryClient, { labels });
    await nextTick();
    labelButton('bug').click();
    await nextTick();

    const otherLabels = [
      {
        id: 'label-campaign',
        name: 'campaign',
        description: '',
        color: '#8b5cf6',
        icon_url: null,
        project_id: 'other-project-uuid',
      },
    ];
    await wrapper.setProps({ projectId: 'other-project-uuid', labels: otherLabels });
    await nextTick();

    expect(labelButton('campaign').getAttribute('aria-pressed')).toBe('false');
    await new DOMWrapper(getTitleInput()).setValue('切替後の作成');
    getForm().dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await flushPromises();

    expect(mutateAsync).toHaveBeenCalledTimes(1);
    expect(mutateAsync.mock.calls[0][0].params.path.project_id).toBe('other-project-uuid');
    expect(mutateAsync.mock.calls[0][0].body.label_ids).toBeUndefined();
    wrapper.unmount();
  });

  it('ラベル一覧の再取得で消えた ID は選択から外れて送信されない', async () => {
    const wrapper = mountDialog(queryClient, { labels });
    await nextTick();
    await new DOMWrapper(getTitleInput()).setValue('削除済みラベル');
    labelButton('bug').click();
    labelButton('feature').click();
    await nextTick();

    await wrapper.setProps({ labels: labels.filter((label) => label.id !== 'label-bug') });
    await nextTick();

    getForm().dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await flushPromises();

    expect(mutateAsync.mock.calls[0][0].body.label_ids).toEqual(['label-feature']);
    wrapper.unmount();
  });

  it('labels が undefined（ロード中・エラー）になっても選択は保持される', async () => {
    const wrapper = mountDialog(queryClient, { labels });
    await nextTick();
    await new DOMWrapper(getTitleInput()).setValue('取得中の選択保持');
    labelButton('feature').click();
    await nextTick();

    await wrapper.setProps({ labels: undefined, labelsLoading: true });
    await nextTick();
    await wrapper.setProps({ labels, labelsLoading: false });
    await nextTick();

    expect(labelButton('feature').getAttribute('aria-pressed')).toBe('true');
    getForm().dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await flushPromises();

    expect(mutateAsync.mock.calls[0][0].body.label_ids).toEqual(['label-feature']);
    wrapper.unmount();
  });
});

describe('CreateTaskDialog labels query states', () => {
  let queryClient: QueryClient;

  beforeEach(() => {
    isHydrated.value = true;
    isPending.value = false;
    mutateAsync.mockReset();
    queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false },
        mutations: { retry: false },
      },
    });
  });

  afterEach(() => {
    document.body.innerHTML = '';
  });

  function labelsGroup() {
    return document.body.querySelector('[role="group"][aria-labelledby="task-labels-label"]');
  }

  it('ロード中はラベル欄にロード中表示を出す', async () => {
    const wrapper = mountDialog(queryClient, { labelsLoading: true });
    await nextTick();

    const group = labelsGroup();
    expect(group).not.toBeNull();
    expect(group?.textContent).toContain('ラベルを読み込み中');
    expect(group?.querySelector('button[aria-pressed]')).toBeNull();
    wrapper.unmount();
  });

  it('取得エラーはエラー表示と再試行を出し、正常な空一覧と区別する', async () => {
    const wrapper = mountDialog(queryClient, { labelsError: true });
    await nextTick();

    const group = labelsGroup();
    expect(group?.textContent).toContain('ラベルの取得に失敗しました');
    const retry = Array.from(group?.querySelectorAll('button') ?? []).find(
      (el) => el.textContent?.trim() === '再試行',
    );
    if (!retry) throw new Error('retry button not found');
    retry.click();
    await nextTick();
    expect(wrapper.emitted('retryLabels')).toHaveLength(1);
    wrapper.unmount();
  });

  it('正常な 0 件ではラベル欄を表示しない', async () => {
    const wrapper = mountDialog(queryClient, { labels: [] });
    await nextTick();

    expect(labelsGroup()).toBeNull();
    wrapper.unmount();
  });

  it('ラベル欄は role=group で「ラベル」見出しに関連付く', async () => {
    const wrapper = mountDialog(queryClient, { labels });
    await nextTick();

    const group = labelsGroup();
    expect(group).not.toBeNull();
    const heading = document.getElementById('task-labels-label');
    expect(heading?.textContent).toContain('ラベル');
    expect(group?.contains(heading)).toBe(true);
    wrapper.unmount();
  });
});
