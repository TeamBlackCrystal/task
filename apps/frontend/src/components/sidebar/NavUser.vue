<script setup lang="ts">
import {
  PhSealCheck,
  PhBell,
  PhCaretUpDown,
  PhCreditCard,
  PhSignOut,
  PhSparkle,
} from '@phosphor-icons/vue';

import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import {
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  useSidebar,
} from '@/components/ui/sidebar';
import { computed } from 'vue';
import { avatarInitials } from '@/lib/initials';
import { shouldCloseSidebarOnNavigate } from '@/components/sidebar/sidebar-navigation';

const props = defineProps<{
  user: {
    name: string;
    email: string;
    avatar: string;
  };
  onLogout?: () => void | Promise<void>;
}>();

const { isMobile, setOpenMobile } = useSidebar();
const initials = computed(() => avatarInitials(props.user.name));

/**
 * Account はナビのリンクと同じ普通の `<a>` で、vike のクライアントルーティングが
 * 処理する。モバイルではサイドバーがページに重なるので、閉じないと遷移先の
 * /settings/profile が覆われたままになる。
 *
 * AppSidebar が SidebarContent に置いたイベント委譲では拾えない。このメニューは
 * DropdownMenuPortal でサイドバーの外へ出るため、クリックがサイドバーへ伝播しない。
 * 判定だけ委譲と同じ関数を使い、閉じる処理をここで持つ。
 */
function closeOnNavigate(event: MouseEvent) {
  if (shouldCloseSidebarOnNavigate(event, isMobile.value)) setOpenMobile(false);
}
</script>

<template>
  <SidebarMenu>
    <SidebarMenuItem>
      <DropdownMenu>
        <DropdownMenuTrigger as-child>
          <SidebarMenuButton
            size="lg"
            class="data-[state=open]:bg-sidebar-accent data-[state=open]:text-sidebar-accent-foreground"
          >
            <Avatar class="h-8 w-8 rounded-lg">
              <AvatarImage :src="user.avatar" :alt="user.name" />
              <AvatarFallback class="rounded-lg">{{ initials }}</AvatarFallback>
            </Avatar>
            <div class="grid flex-1 text-left text-sm leading-tight">
              <span class="truncate font-medium">{{ user.name }}</span>
              <span class="truncate text-xs">{{ user.email }}</span>
            </div>
            <PhCaretUpDown class="ml-auto size-4" />
          </SidebarMenuButton>
        </DropdownMenuTrigger>
        <DropdownMenuContent
          class="w-(--reka-dropdown-menu-trigger-width) min-w-56 rounded-lg"
          :side="isMobile ? 'bottom' : 'right'"
          align="end"
          :side-offset="4"
        >
          <DropdownMenuLabel class="p-0 font-normal">
            <div class="flex items-center gap-2 px-1 py-1.5 text-left text-sm">
              <Avatar class="h-8 w-8 rounded-lg">
                <AvatarImage :src="user.avatar" :alt="user.name" />
                <AvatarFallback class="rounded-lg">{{ initials }}</AvatarFallback>
              </Avatar>
              <div class="grid flex-1 text-left text-sm leading-tight">
                <span class="truncate font-semibold">{{ user.name }}</span>
                <span class="truncate text-xs">{{ user.email }}</span>
              </div>
            </div>
          </DropdownMenuLabel>
          <DropdownMenuSeparator />
          <DropdownMenuGroup>
            <DropdownMenuItem>
              <PhSparkle />
              Upgrade to Pro
            </DropdownMenuItem>
          </DropdownMenuGroup>
          <DropdownMenuSeparator />
          <DropdownMenuGroup>
            <DropdownMenuItem as-child>
              <a href="/settings/profile" @click="closeOnNavigate">
                <PhSealCheck />
                Account
              </a>
            </DropdownMenuItem>
            <DropdownMenuItem>
              <PhCreditCard />
              Billing
            </DropdownMenuItem>
            <DropdownMenuItem>
              <PhBell />
              Notifications
            </DropdownMenuItem>
          </DropdownMenuGroup>
          <DropdownMenuSeparator />
          <DropdownMenuItem @click="onLogout?.()">
            <PhSignOut />
            Log out
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </SidebarMenuItem>
  </SidebarMenu>
</template>
