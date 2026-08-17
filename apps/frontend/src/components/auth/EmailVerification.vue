<script setup lang="ts">
import { usePageContext } from 'vike-vue/usePageContext';
import { onMounted, ref } from 'vue';
import { Button } from '@/components/ui/button';
import { useVerifyEmailMutation } from '@/lib/api-vue-query';

defineOptions({ name: 'EmailVerification' });

const pageContext = usePageContext();
// メール内リンクは /verify-email?token=… （backend の verification_email_delivery が生成）
const token =
  (pageContext as { urlParsed?: { search?: Record<string, string> } }).urlParsed?.search?.token ??
  null;

const verifyMutation = useVerifyEmailMutation();
// トークンは一度きりで消費される。SSR で叩かないよう onMounted（クライアント）でのみ実行する
const state = ref<'verifying' | 'verified' | 'invalid'>(token ? 'verifying' : 'invalid');

onMounted(async () => {
  if (!token) return;
  try {
    await verifyMutation.mutateAsync({ body: { token }, parseAs: 'text' });
    state.value = 'verified';
  } catch {
    state.value = 'invalid';
  }
});
</script>

<template>
  <div v-if="state === 'verifying'" class="text-muted-foreground p-6 text-center text-sm md:p-8">
    リンクを確認しています…
  </div>
  <div
    v-else-if="state === 'verified'"
    class="flex flex-col items-center gap-4 p-6 text-center md:p-8"
  >
    <h1 class="text-2xl font-bold">メールアドレスを確認しました</h1>
    <p class="text-muted-foreground text-sm">サインインしてご利用ください。</p>
    <a href="/signin">
      <Button>サインインページへ</Button>
    </a>
  </div>
  <div v-else class="flex flex-col items-center gap-4 p-6 text-center md:p-8">
    <h1 class="text-2xl font-bold">リンクが無効です</h1>
    <p class="text-muted-foreground text-sm">
      確認用のリンクが無効か、有効期限が切れています。すでに確認が済んでいる場合もこの表示になります。サインインできないときは、サインインページから確認メールを再送してください。
    </p>
    <a href="/signin">
      <Button variant="outline">サインインページへ</Button>
    </a>
  </div>
</template>
