<script setup lang="ts">
import { Button } from '@/components/ui/button';
import { activityText, relativeTime, type ActivityItem } from '@/lib/task-activity';

/**
 * タスクの操作履歴。
 *
 * 参照どおり、1 件を「• 本文 …… 時刻（右寄せ）」の箇条書きで出す。コメントのように
 * カードで囲わないことで、履歴とコメントが見分けられる。
 *
 * 取得失敗はこの中で倒す（再試行ボタンを出す）。履歴はコメントと独立した口なので、
 * 片方が落ちてももう片方は読み書きできる。
 */
defineProps<{
  activities: ActivityItem[];
  loading?: boolean;
  error?: boolean;
  onRetry?: () => void;
}>();
</script>

<template>
  <div data-task-activity-feed>
    <p v-if="loading" class="text-sm text-muted-foreground">読み込み中…</p>

    <div v-else-if="error" class="flex items-center gap-2">
      <p class="text-sm text-destructive">履歴を読み込めませんでした</p>
      <Button v-if="onRetry" type="button" variant="outline" size="sm" @click="onRetry">
        再試行
      </Button>
    </div>

    <ul v-else-if="activities.length" class="flex flex-col gap-2">
      <!-- 履歴はシステムの記録なので、本文（コメント）より薄い文字色にする -->
      <li
        v-for="item in activities"
        :key="item.id"
        class="flex items-start gap-2 text-sm text-muted-foreground"
      >
        <span class="mt-2 size-1 shrink-0 rounded-full bg-muted-foreground/60" aria-hidden="true" />
        <span class="min-w-0 flex-1">
          {{ item.user?.name ?? 'システム' }}が{{ activityText(item) }}
        </span>
        <span class="shrink-0 whitespace-nowrap text-xs text-muted-foreground">
          {{ relativeTime(item.created_at) }}
        </span>
      </li>
    </ul>
  </div>
</template>
