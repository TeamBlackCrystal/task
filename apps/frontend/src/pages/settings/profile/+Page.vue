<script setup lang="ts">
import { usePageContext } from 'vike-vue/usePageContext';
import AccountSettingsNav from '@/components/settings/AccountSettingsNav.vue';
import ProfileForm from '@/components/settings/ProfileForm.vue';
import { useMeQuery } from '@/lib/api-vue-query';

const pageContext = usePageContext();
const meQuery = useMeQuery();
</script>

<template>
  <div class="mx-auto flex w-full max-w-4xl flex-col gap-6 py-6">
    <header class="flex flex-col gap-1">
      <h1 class="text-2xl font-bold">アカウント設定</h1>
      <p class="text-muted-foreground text-sm">個人アカウントの情報を管理します。</p>
    </header>

    <div class="flex flex-col gap-8 md:flex-row">
      <aside class="md:w-56 md:shrink-0">
        <AccountSettingsNav :current-path="pageContext.urlPathname" />
      </aside>

      <section class="flex min-w-0 flex-1 flex-col gap-4">
        <div class="flex flex-col gap-1">
          <h2 class="text-lg font-semibold">プロフィール</h2>
          <p class="text-muted-foreground text-sm">
            ここで設定した内容は、同じテナントのメンバーに表示されます。
          </p>
        </div>

        <p v-if="meQuery.isPending.value" class="text-muted-foreground text-sm">読み込み中…</p>
        <p v-else-if="!meQuery.data.value" class="text-destructive text-sm">
          プロフィールを読み込めませんでした。
        </p>
        <ProfileForm v-else :user="meQuery.data.value" />
      </section>
    </div>
  </div>
</template>
