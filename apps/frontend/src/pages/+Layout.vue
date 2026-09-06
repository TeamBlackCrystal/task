<!-- https://vike.dev/Layout -->

<script setup lang="ts">
import AppSidebarSkeleton from '@/components/sidebar/AppSidebarSkeleton.vue';
import {
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbPage,
  BreadcrumbSeparator,
} from '@/components/ui/breadcrumb';
import { Separator } from '@/components/ui/separator';
import { SidebarInset, SidebarProvider, SidebarTrigger } from '@/components/ui/sidebar';
import EmailNotVerified from '@/components/auth/EmailNotVerified.vue';
import { useAuthSession } from '@/composables/useAuthSession';
import { ClientOnly } from 'vike-vue/ClientOnly';
import { usePageContext } from 'vike-vue/usePageContext';
import { computed, defineAsyncComponent } from 'vue';

const TanStackDevtools = import.meta.env.DEV
  ? defineAsyncComponent(() => import('@/components/devtools/TanStackDevtoolsClient.vue'))
  : null;
const isDev = import.meta.env.DEV;

const pageContext = usePageContext();
// サインイン前に開くページ。認証ガードを外さないと /signin へ飛ばされる
const isAuthPage = computed(() =>
  ['/signin', '/signup', '/auth/reset-password', '/verify-email'].includes(pageContext.urlPathname),
);

const { meQuery, logout } = useAuthSession({
  guard: computed(() => !isAuthPage.value),
});

const isEmailVerified = computed(() => meQuery.data.value?.email_verified ?? true);

const AppSidebar = defineAsyncComponent(() => import('@/components/sidebar/AppSidebar.vue'));
const AppHeader = defineAsyncComponent(() => import('@/components/header/AppHeader.vue'));
</script>

<template>
  <ClientOnly v-if="isDev && TanStackDevtools">
    <component :is="TanStackDevtools" />
  </ClientOnly>
  <slot v-if="isAuthPage" />
  <div
    v-else-if="meQuery.isPending.value"
    class="flex min-h-svh items-center justify-center text-muted-foreground text-sm"
  >
    読み込み中…
  </div>
  <EmailNotVerified
    v-else-if="meQuery.isSuccess.value && !isEmailVerified"
    :email="meQuery.data.value!.email"
    @logout="logout"
  />
  <!--
    テナントとアカウントは全幅のヘッダーに置く（サイドバーの上下ではなく）。
    SidebarProvider は横並びなので、縦積みにして 1 段目をヘッダー、2 段目を
    サイドバー + 本文にする。min-h-0 は min-h-svh を twMerge で上書きするためのもの。
  -->
  <SidebarProvider v-else-if="meQuery.isSuccess.value" class="min-h-0 h-svh flex-col">
    <AppHeader />
    <div class="flex min-h-0 w-full flex-1">
      <Suspense>
        <AppSidebar desktop-top-offset="3rem" />
        <template #fallback>
          <AppSidebarSkeleton desktop-top-offset="3rem" />
        </template>
      </Suspense>
      <SidebarInset>
        <header
          class="flex h-16 shrink-0 items-center gap-2 transition-[width,height] ease-linear group-has-data-[collapsible=icon]/sidebar-wrapper:h-12"
        >
          <div class="flex items-center gap-2 px-4">
            <SidebarTrigger class="-ml-1" />
            <Separator orientation="vertical" class="mr-2 data-[orientation=vertical]:h-4" />
            <Breadcrumb>
              <BreadcrumbList>
                <BreadcrumbItem class="hidden md:block">
                  <BreadcrumbLink href="#"> ToDo </BreadcrumbLink>
                </BreadcrumbItem>
                <BreadcrumbSeparator class="hidden md:block" />
                <BreadcrumbItem>
                  <BreadcrumbPage>いい感じにする</BreadcrumbPage>
                </BreadcrumbItem>
              </BreadcrumbList>
            </Breadcrumb>
          </div>
        </header>
        <div class="flex flex-1 flex-col gap-4 p-4 pt-0">
          <slot />
        </div>
      </SidebarInset>
    </div>
  </SidebarProvider>
</template>
