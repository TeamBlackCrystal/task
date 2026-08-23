<script setup lang="ts">
import { computed } from 'vue';
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar';
import { avatarInitials } from '@/lib/initials';
import type { components } from '@/generated/api';

type UserSummary = components['schemas']['UserSummary'];

const props = withDefaults(
  defineProps<{
    /** 担当ユーザー一覧（表示順は API 応答順） */
    users: UserSummary[];
    /** 重ね表示する最大数（超過分は +N チップ） */
    maxDisplay?: number;
    /** true のとき「先頭名 + 他N名」テキストを出さずアバターのみ表示する */
    hideNames?: boolean;
  }>(),
  {
    maxDisplay: 3,
    hideNames: false,
  },
);

const visibleUsers = computed(() => props.users.slice(0, props.maxDisplay));
/** maxDisplay を超えて非表示のアバター数。+N チップと「他N名」テキストの両方で同一の値を使う。 */
const overflowCount = computed(() => Math.max(0, props.users.length - props.maxDisplay));
const firstUser = computed(() => props.users[0]);

function initials(username: string) {
  return avatarInitials(username, 1);
}
</script>

<template>
  <div class="flex items-center gap-1.5">
    <!-- 重ねアバター群 -->
    <div class="flex -space-x-2">
      <div
        v-for="user in visibleUsers"
        :key="user.id"
        class="size-7 rounded-full ring-2 ring-background"
      >
        <Avatar class="size-full">
          <AvatarImage v-if="user.avatar_url" :src="user.avatar_url" :alt="user.username" />
          <AvatarFallback class="text-[10px] bg-muted text-muted-foreground">
            {{ initials(user.username) }}
          </AvatarFallback>
        </Avatar>
      </div>
      <!-- +N オーバーフローチップ（comp-409 準拠） -->
      <div
        v-if="overflowCount > 0"
        class="size-7 rounded-full bg-muted text-muted-foreground text-[10px] font-medium flex items-center justify-center ring-2 ring-background"
      >
        +{{ overflowCount }}
      </div>
    </div>
    <!-- 先頭名 + 他N名 テキスト（殿指示により維持。N は overflowCount と同義）。
         hideNames 指定時はアバターのみ（タスク詳細の担当者欄で使用） -->
    <span v-if="!hideNames" class="text-xs truncate max-w-28 text-muted-foreground">
      <template v-if="users.length === 1">{{ firstUser?.username }}</template>
      <template v-else-if="overflowCount > 0"
        >{{ firstUser?.username }} 他{{ overflowCount }}名</template
      >
      <template v-else>{{ firstUser?.username }}</template>
    </span>
  </div>
</template>
