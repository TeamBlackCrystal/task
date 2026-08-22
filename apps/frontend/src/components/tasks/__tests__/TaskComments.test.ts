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

  it('一覧の読み込み失敗はリスト位置でエラー表示しつつ、投稿フォームは出したままにする', async () => {
    const onRetry = vi.fn();
    const wrapper = mountComments({ listError: true, onRetry });
    expect(wrapper.text()).toContain('コメントを読み込めませんでした');
    // 一覧の GET が失敗しても POST は独立に成功しうるため、書く導線は残す
    expect(wrapper.find('form').exists()).toBe(true);

    const retryButton = wrapper.findAll('button').find((button) => button.text() === '再試行');
    expect(retryButton).toBeDefined();
    await retryButton!.trigger('click');
    expect(onRetry).toHaveBeenCalledTimes(1);
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
    const wrapper = mountComments({
      threads: [thread('c-1', '元の本文')],
      currentUserId: user.id,
      onUpdate,
    });

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
    // 成功（backend が許可）したら確認 UI を閉じる
    expect(wrapper.text()).not.toContain('このコメントを削除しますか？');
  });

  it('削除が backend に拒否されたら確認 UI を残し、拒否理由を対象コメントの中に出す', async () => {
    const onDelete = vi.fn(async () => false);
    const wrapper = mountComments({ threads: [thread('c-1', '消せないコメント')], onDelete });

    await wrapper.get('button[aria-label="コメントを削除"]').trigger('click');
    const confirmButton = wrapper.findAll('button').find((button) => button.text() === '削除する');
    await confirmButton!.trigger('click');
    await flushPromises();

    expect(onDelete).toHaveBeenCalledTimes(1);
    // 失敗時は確認 UI を残す（成功系の「閉じる」との対照）
    expect(wrapper.text()).toContain('このコメントを削除しますか？');

    await wrapper.setProps({
      deleteError: 'コメントを削除できませんでした（forbidden）',
      deleteErrorCommentId: 'c-1',
    });
    expect(wrapper.get('[data-task-comment]').text()).toContain(
      'コメントを削除できませんでした（forbidden）',
    );
  });

  it('返信フォームを開いたままスレッド削除に成功したらフォームを閉じる', async () => {
    const onDelete = vi.fn(async () => true);
    const wrapper = mountComments({ threads: [thread('c-1', '消すスレッド')], onDelete });

    const replyOpenButton = wrapper.findAll('button').find((button) => button.text() === '返信');
    await replyOpenButton!.trigger('click');
    expect(wrapper.find('textarea[aria-label="返信"]').exists()).toBe(true);

    await wrapper.get('button[aria-label="コメントを削除"]').trigger('click');
    const confirmButton = wrapper.findAll('button').find((button) => button.text() === '削除する');
    await confirmButton!.trigger('click');
    await flushPromises();

    // 削除済みスレッドへは返信できない（backend が 400 で弾く）ため、フォームは残さない
    expect(wrapper.find('textarea[aria-label="返信"]').exists()).toBe(false);
  });

  it('編集 UI の開閉で前回の失敗表示を消す（返信フォームと同型）', async () => {
    const onClearUpdateError = vi.fn();
    const wrapper = mountComments({
      threads: [thread('c-1', '本文')],
      currentUserId: user.id,
      onClearUpdateError,
      updateError: 'コメントを更新できませんでした（forbidden）',
      updateErrorCommentId: 'c-1',
    });

    await wrapper.get('button[aria-label="コメントを編集"]').trigger('click');
    expect(onClearUpdateError).toHaveBeenCalledTimes(1);

    await wrapper.setProps({ updateError: null, updateErrorCommentId: null });
    const cancelButton = wrapper.findAll('button').find((button) => button.text() === 'キャンセル');
    await cancelButton!.trigger('click');
    expect(onClearUpdateError).toHaveBeenCalledTimes(2);
  });

  it('削除確認の開閉で前回の失敗表示を消す（返信フォームと同型）', async () => {
    const onClearDeleteError = vi.fn();
    const wrapper = mountComments({
      threads: [thread('c-1', '本文')],
      onClearDeleteError,
      deleteError: 'コメントを削除できませんでした（forbidden）',
      deleteErrorCommentId: 'c-1',
    });

    await wrapper.get('button[aria-label="コメントを削除"]').trigger('click');
    expect(onClearDeleteError).toHaveBeenCalledTimes(1);

    await wrapper.setProps({ deleteError: null, deleteErrorCommentId: null });
    const cancelButton = wrapper.findAll('button').find((button) => button.text() === 'キャンセル');
    await cancelButton!.trigger('click');
    expect(onClearDeleteError).toHaveBeenCalledTimes(2);
  });

  it('削除済みコメントはプレースホルダを出し、編集・削除ボタンを出さない', () => {
    const wrapper = mountComments({
      threads: [thread('c-1', null, { is_deleted: true })],
    });

    expect(wrapper.text()).toContain('削除されたコメント');
    expect(wrapper.text()).not.toContain('(編集済み)');
    expect(wrapper.find('button[aria-label="コメントを編集"]').exists()).toBe(false);
    expect(wrapper.find('button[aria-label="コメントを削除"]').exists()).toBe(false);
  });

  it('削除済みでも updated_at が異なっていても (編集済み) は出さない', () => {
    const wrapper = mountComments({
      threads: [
        thread('c-1', null, {
          is_deleted: true,
          updated_at: '2026-08-19T02:00:00Z',
        }),
      ],
    });
    expect(wrapper.text()).not.toContain('(編集済み)');
  });

  it('編集済みコメントには (編集済み) を出す', () => {
    const wrapper = mountComments({
      threads: [thread('c-1', '本文', { updated_at: '2026-08-19T02:00:00Z' })],
    });
    expect(wrapper.text()).toContain('(編集済み)');
  });

  it('削除済みスレッドには返信ボタンを出さない（backend が必ず 400 で弾く導線）', () => {
    const wrapper = mountComments({
      threads: [thread('c-1', null, { is_deleted: true }), thread('c-2', '生きているスレッド')],
    });

    // 返信ボタンは生きているスレッドの 1 つだけ
    const replyButtons = wrapper.findAll('button').filter((button) => button.text() === '返信');
    expect(replyButtons).toHaveLength(1);
  });

  it('編集ボタンは投稿者本人のコメントにだけ出す（backend が本人以外を必ず 403 にする）', () => {
    const other = { id: 'user-2', name: '佐藤花子' };
    const wrapper = mountComments({
      threads: [thread('c-1', '自分のコメント'), thread('c-2', '他人のコメント', { user: other })],
      currentUserId: user.id,
    });

    const editButtons = wrapper.findAll('button[aria-label="コメントを編集"]');
    expect(editButtons).toHaveLength(1);
    // 削除ボタンはテナントオーナーも許されるため全コメントに出る
    expect(wrapper.findAll('button[aria-label="コメントを削除"]')).toHaveLength(2);
  });

  it('返信の失敗は対象スレッドの返信フォーム直下に出す', async () => {
    const wrapper = mountComments({
      threads: [thread('c-1', '親コメント')],
      onSubmit: vi.fn(async () => false),
    });

    const replyOpenButton = wrapper.findAll('button').find((button) => button.text() === '返信');
    await replyOpenButton!.trigger('click');
    await wrapper.setProps({
      replyError: '返信を投稿できませんでした（bad-request）',
      replyErrorThreadId: 'c-1',
    });

    const replyForm = wrapper.find('textarea[aria-label="返信"]');
    expect(replyForm.exists()).toBe(true);
    expect(wrapper.text()).toContain('返信を投稿できませんでした（bad-request）');
  });

  it('返信フォームを開き直すとき前回の失敗表示を消す', async () => {
    const onClearReplyError = vi.fn();
    const wrapper = mountComments({
      threads: [thread('c-1', '親コメント')],
      onClearReplyError,
      replyError: '返信を投稿できませんでした（bad-request）',
      replyErrorThreadId: 'c-1',
    });

    const replyOpenButton = wrapper.findAll('button').find((button) => button.text() === '返信');
    await replyOpenButton!.trigger('click');
    expect(onClearReplyError).toHaveBeenCalledTimes(1);

    await wrapper.setProps({ replyError: null, replyErrorThreadId: null });
    const cancelButton = wrapper.findAll('button').find((button) => button.text() === 'キャンセル');
    await cancelButton!.trigger('click');
    expect(onClearReplyError).toHaveBeenCalledTimes(2);
  });

  it('更新・削除の失敗は対象コメントの中に出す', async () => {
    const other = { id: 'user-2', name: '佐藤花子' };
    const wrapper = mountComments({
      threads: [thread('c-1', '対象'), thread('c-2', '無関係', { user: other })],
      updateError: 'コメントを更新できませんでした（forbidden）',
      updateErrorCommentId: 'c-1',
    });

    const items = wrapper.findAll('[data-task-comment]');
    expect(items[0].text()).toContain('コメントを更新できませんでした（forbidden）');
    expect(items[1].text()).not.toContain('コメントを更新できませんでした');
  });
});
