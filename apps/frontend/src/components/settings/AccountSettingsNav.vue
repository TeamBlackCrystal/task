<script setup lang="ts">
import { PhBell, PhDevices, PhKey, PhSlidersHorizontal, PhUser } from '@phosphor-icons/vue';
import type { Component } from 'vue';

type NavItem = {
  title: string;
  icon: Component;
  /** 未実装の項目は href を持たず、押せない見出しとして並べる。 */
  href?: string;
};

const items: NavItem[] = [
  { title: 'プロフィール', icon: PhUser, href: '/settings/profile' },
  { title: '環境設定', icon: PhSlidersHorizontal },
  { title: '通知', icon: PhBell },
  { title: 'アクセストークン', icon: PhKey },
  { title: 'セッション', icon: PhDevices },
];

defineProps<{ currentPath: string }>();
</script>

<template>
  <nav class="flex flex-col gap-1" aria-label="アカウント設定">
    <template v-for="item in items" :key="item.title">
      <a
        v-if="item.href"
        :href="item.href"
        :aria-current="item.href === currentPath ? 'page' : undefined"
        class="flex items-center gap-2 rounded-md px-3 py-2 text-sm aria-[current=page]:bg-accent aria-[current=page]:font-medium hover:bg-accent/50"
      >
        <component :is="item.icon" class="size-4 shrink-0" />
        <span>{{ item.title }}</span>
      </a>
      <span
        v-else
        aria-disabled="true"
        title="未実装です"
        class="text-muted-foreground/50 flex cursor-not-allowed items-center gap-2 rounded-md px-3 py-2 text-sm"
      >
        <component :is="item.icon" class="size-4 shrink-0" />
        <span>{{ item.title }}</span>
      </span>
    </template>
  </nav>
</template>
