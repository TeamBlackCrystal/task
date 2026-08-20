import { afterEach, describe, expect, it, vi } from 'vitest';
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils';
import TaskComments from '../TaskComments.vue';
import type { CommentThread } from '@/composables/useTaskComments';

enableAutoUnmount(afterEach);

const user = { id: 'user-1', name: '田中太郎' };

function thread(id: string, body: string | null, overrides: Partial<CommentThread> = {}) {
  return {
    id,
    body,
    is_deleted: false,
    created_at: '2026-08-19T00:00:00Z',
    updated_at: '2026-08-19T00:00:00Z',
    user,
    replies: [],
    ...overrides,
  } satisfies CommentThread;
}

function mountComments(props: Partial<InstanceType<typeof TaskComments>['$props']> = {}) {
  return mount(TaskComments, {
    props: {
      threads: [],
      onSubmit: vi.fn(async () => true),
      onUpdate: vi.fn(async () => true),
      onDelete: vi.fn(async () => true),
      ...props,
    },
  });
}

describe('TaskComments', () => {
  it('スレッドと返信を素テキストで表示し、HTML はエスケープされたまま出す', () => {
    const wrapper = mountComments({
      threads: [
        thread('c-1', '一行目\n<script>alert(1)</script>', {
          replies: [
            {
              id: 'r-1',
              body: '返信本文',
              is_deleted: false,
              created_at: '2026-08-19T01:00:00Z',
              updated_at: '2026-08-19T01:00:00Z',
              user,
            },
          ],
        }),
      ],
    });

    const bodies = wrapper.findAll('[data-task-comment] p.whitespace-pre-wrap');
    expect(bodies).toHaveLength(2);
    // v-html ではないため、タグはテキストとしてそのまま残る
    expect(bodies[0].text()).toContain('<script>alert(1)</script>');
    expect(wrapper.find('script').exists()).toBe(false);
    expect(bodies[1].text()).toBe('返信本文');
  });

  it('コメントが無いときは空メッセージを出す', () => {
    const wrapper = mountComments();
    expect(wrapper.text()).toContain('コメントはまだありません');
  });

  it('一覧の読み込み失敗はコメント節の中だけでエラー表示し、投稿フォームも出さない', () => {
    const wrapper = mountComments({ listError: true });
    expect(wrapper.text()).toContain('コメントを読み込めませんでした');
    expect(wrapper.find('form').exists()).toBe(false);
  });

  it('投稿は onSubmit を呼び、成功したら下書きを消す', async () => {
    const onSubmit = vi.fn(async () => true);
    const wrapper = mountComments({ onSubmit });

    const textarea = wrapper.get('textarea[aria-label="コメントを入力"]');
    await textarea.setValue('新しいコメント');
    await wrapper.get('form').trigger('submit');
    await flushPromises();

    expect(onSubmit).toHaveBeenCalledWith('新しいコメント');
    expect((textarea.element as HTMLTextAreaElement).value).toBe('');
  });

  it('投稿が失敗（拒否）したら下書きを残し、submitError を表示する', async () => {
    const onSubmit = vi.fn(async () => false);
    const wrapper = mountComments({ onSubmit });

    const textarea = wrapper.get('textarea[aria-label="コメントを入力"]');
    await textarea.setValue('拒否されるコメント');
    await wrapper.get('form').trigger('submit');
    await flushPromises();

    expect((textarea.element as HTMLTextAreaElement).value).toBe('拒否されるコメント');

    await wrapper.setProps({ submitError: 'コメントを投稿できませんでした（forbidden）' });
    expect(wrapper.text()).toContain('コメントを投稿できませんでした（forbidden）');
  });

  it('返信フォームは parent としてスレッド ID を渡す', async () => {
    const onSubmit = vi.fn(async () => true);
    const wrapper = mountComments({ threads: [thread('c-1', '親コメント')], onSubmit });

    const replyOpenButton = wrapper.findAll('button').find((button) => button.text() === '返信');
    expect(replyOpenButton).toBeDefined();
    await replyOpenButton!.trigger('click');
    const replyArea = wrapper.get('textarea[aria-label="返信"]');
    await replyArea.setValue('返信します');
    const replyButton = wrapper.findAll('button').find((button) => button.text() === '返信する');
    expect(replyButton).toBeDefined();
    await replyButton!.trigger('click');
    await flushPromises();

    expect(onSubmit).toHaveBeenCalledWith('返信します', 'c-1');
    // 成功で返信フォームは閉じる
    expect(wrapper.find('textarea[aria-label="返信"]').exists()).toBe(false);
  });

  it('編集は onUpdate をコメント ID と新本文で呼ぶ', async () => {
    const onUpdate = vi.fn(async () => true);
    const wrapper = mountComments({ threads: [thread('c-1', '元の本文')], onUpdate });

    await wrapper.get('button[aria-label="コメントを編集"]').trigger('click');
    const editArea = wrapper.get('textarea[aria-label="コメントを編集"]');
    expect((editArea.element as HTMLTextAreaElement).value).toBe('元の本文');
    await editArea.setValue('直した本文');
    const saveButton = wrapper.findAll('button').find((button) => button.text() === '保存');
    await saveButton!.trigger('click');
    await flushPromises();

    expect(onUpdate).toHaveBeenCalledWith('c-1', '直した本文');
    // 成功で編集 UI は閉じ、表示に戻る
    expect(wrapper.find('textarea[aria-label="コメントを編集"]').exists()).toBe(false);
  });

  it('削除は確認を挟んでから onDelete を呼ぶ', async () => {
    const onDelete = vi.fn(async () => true);
    const wrapper = mountComments({ threads: [thread('c-1', '消すコメント')], onDelete });

    await wrapper.get('button[aria-label="コメントを削除"]').trigger('click');
    expect(onDelete).not.toHaveBeenCalled();
    expect(wrapper.text()).toContain('このコメントを削除しますか？');

    const confirmButton = wrapper.findAll('button').find((button) => button.text() === '削除する');
    await confirmButton!.trigger('click');
    await flushPromises();

    expect(onDelete).toHaveBeenCalledTimes(1);
  });

  it('削除済みコメントはプレースホルダを出し、編集・削除ボタンを出さない', () => {
    const wrapper = mountComments({
      threads: [thread('c-1', null, { is_deleted: true })],
    });

    expect(wrapper.text()).toContain('削除されたコメント');
    expect(wrapper.find('button[aria-label="コメントを編集"]').exists()).toBe(false);
    expect(wrapper.find('button[aria-label="コメントを削除"]').exists()).toBe(false);
  });

  it('編集済みコメントには (編集済み) を出す', () => {
    const wrapper = mountComments({
      threads: [thread('c-1', '本文', { updated_at: '2026-08-19T02:00:00Z' })],
    });
    expect(wrapper.text()).toContain('(編集済み)');
  });
});
