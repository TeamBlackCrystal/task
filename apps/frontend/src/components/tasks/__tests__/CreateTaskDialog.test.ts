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
});
