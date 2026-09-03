<script setup lang="ts">
import { computed, ref } from 'vue';
import { PhUserPlus } from '@phosphor-icons/vue';

import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { avatarInitials } from '@/lib/initials';

export type PickableMember = { id: string; username: string; avatar_url?: string | null };

/**
 * 担当者の選択。一覧の行・詳細・作成行の 3 箇所で使うので切り出す。
 *
 * 担当は複数付けられるのでチェックボックス。人数が増えると探せなくなるため、
 * 名前で絞り込む窓を上に置く。
 */
const props = withDefaults(
  defineProps<{
    members: PickableMember[];
    /** 選択済みのユーザー（表示にも使うので id だけでなく実体で受ける） */
    selected: PickableMember[];
    disabled?: boolean;
    /** 重ねて出すアバターの最大数。超過分は +N */
    maxDisplay?: number;
  }>(),
  { maxDisplay: 3 },
);

const emit = defineEmits<{
  toggle: [userId: string, checked: boolean];
}>();

const query = ref('');
const visibleMembers = computed(() => {
  const keyword = query.value.trim().toLowerCase();
  if (!keyword) return props.members;
  return props.members.filter((member) => member.username.toLowerCase().includes(keyword));
});

const shown = computed(() => props.selected.slice(0, props.maxDisplay));
const hiddenCount = computed(() => Math.max(0, props.selected.length - props.maxDisplay));
const label = computed(() =>
  props.selected.length
    ? `担当者: ${props.selected.map((user) => user.username).join('、')}`
    : '担当者を割り当てる',
);

function isSelected(userId: string) {
  return props.selected.some((user) => user.id === userId);
}
</script>

<template>
  <DropdownMenu>
    <DropdownMenuTrigger as-child>
      <Button
        type="button"
        variant="ghost"
        size="sm"
        class="h-7 gap-0 px-1"
        :disabled="disabled"
        :aria-label="label"
      >
        <template v-if="selected.length">
          <Avatar
            v-for="user in shown"
            :key="user.id"
            class="-ml-1.5 size-6 border border-background first:ml-0"
          >
            <AvatarImage v-if="user.avatar_url" :src="user.avatar_url" alt="" />
            <AvatarFallback class="text-[10px]">{{ avatarInitials(user.username) }}</AvatarFallback>
          </Avatar>
          <span v-if="hiddenCount" class="ml-1 text-xs text-muted-foreground">
            +{{ hiddenCount }}
          </span>
        </template>
        <PhUserPlus v-else class="size-4 text-muted-foreground" />
      </Button>
    </DropdownMenuTrigger>

    <DropdownMenuContent align="start" class="w-60">
      <div class="p-1">
        <Input
          v-model="query"
          class="h-8"
          placeholder="メンバーを検索"
          aria-label="メンバーを検索"
        />
      </div>
      <p v-if="!members.length" class="px-2 py-1.5 text-sm text-muted-foreground">
        メンバーがいません
      </p>
      <p v-else-if="!visibleMembers.length" class="px-2 py-1.5 text-sm text-muted-foreground">
        一致するメンバーがいません
      </p>
      <DropdownMenuCheckboxItem
        v-for="member in visibleMembers"
        :key="member.id"
        :model-value="isSelected(member.id)"
        :disabled="disabled"
        @select="(event: Event) => event.preventDefault()"
        @update:model-value="(value) => emit('toggle', member.id, !!value)"
      >
        <Avatar class="size-5">
          <AvatarImage v-if="member.avatar_url" :src="member.avatar_url" alt="" />
          <AvatarFallback class="bg-muted text-[9px] text-muted-foreground">
            {{ avatarInitials(member.username) }}
          </AvatarFallback>
        </Avatar>
        {{ member.username }}
      </DropdownMenuCheckboxItem>
    </DropdownMenuContent>
  </DropdownMenu>
</template>
