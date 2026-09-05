import type { components } from '@/generated/api';

export type ActivityItem = components['schemas']['ActivityItem'];

/**
 * 履歴 1 件の表示文言。
 *
 * `event_type` と `payload` は backend が積んだ生の値なので、画面に出す文へ変換する。
 * 未知の `event_type` は落とさず「操作しました」で出す（履歴が欠けるより、
 * 何か起きたことが分かる方がよい）。
 */
export function activityText(item: ActivityItem): string {
  const payload = (item.payload ?? {}) as Record<string, unknown>;
  const str = (key: string) => (typeof payload[key] === 'string' ? (payload[key] as string) : null);

  switch (item.event_type) {
    case 'task_created':
      return 'タスクを作成しました';
    case 'status_changed': {
      const to = str('to');
      return to ? `ステータスを ${to} に変更しました` : 'ステータスを変更しました';
    }
    case 'priority_changed': {
      const to = str('to');
      return to ? `優先度を ${to} に変更しました` : '優先度を変更しました';
    }
    case 'deadline_changed': {
      const field = str('field') === 'hard_deadline' ? 'ハード期限' : '期限';
      const to = str('to');
      return to ? `${field}を ${to.slice(0, 10)} に変更しました` : `${field}を外しました`;
    }
    case 'assignee_added':
      return '担当者を追加しました';
    case 'assignee_removed':
      return '担当者を外しました';
    case 'relation_added':
      return '関連タスクを追加しました';
    case 'relation_removed':
      return '関連タスクを外しました';
    case 'comment_added':
      return 'コメントしました';
    case 'comment_edited':
      return 'コメントを編集しました';
    case 'comment_deleted':
      return 'コメントを削除しました';
    case 'archived':
      return 'タスクをアーカイブしました';
    default:
      return '操作しました';
  }
}

/** 「1分前」形式。1 日以上は日付にする（履歴は古いものほど絶対時刻の方が読みやすい）。 */
export function relativeTime(iso: string, now: Date = new Date()): string {
  const then = new Date(iso);
  if (Number.isNaN(then.getTime())) return '';
  const diffMs = now.getTime() - then.getTime();
  if (diffMs < 60_000) return 'たった今';
  const minutes = Math.floor(diffMs / 60_000);
  if (minutes < 60) return `${minutes}分前`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}時間前`;
  return then.toLocaleDateString('ja-JP', { year: 'numeric', month: 'numeric', day: 'numeric' });
}
