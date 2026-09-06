<script setup lang="ts">
import type { SidebarProps } from '@/components/ui/sidebar';
import { Skeleton } from '@/components/ui/skeleton';
import { Sidebar, SidebarContent, SidebarRail } from '@/components/ui/sidebar';

const props = defineProps<Pick<SidebarProps, 'desktopTopOffset'>>();
</script>

<template>
  <Sidebar collapsible="icon" :desktop-top-offset="props.desktopTopOffset">
    <SidebarContent class="gap-4 p-2">
      <div class="space-y-2">
        <Skeleton class="h-8 w-full" />
        <Skeleton class="h-8 w-5/6" />
        <Skeleton class="h-8 w-4/6" />
      </div>
      <div class="space-y-2">
        <Skeleton class="h-4 w-20" />
        <Skeleton class="h-8 w-full" />
        <Skeleton class="h-8 w-full" />
        <Skeleton class="h-8 w-3/4" />
      </div>
    </SidebarContent>
    <SidebarRail />
  </Sidebar>
</template>
