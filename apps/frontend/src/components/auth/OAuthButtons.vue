<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { Button } from '@/components/ui/button';
import { Field, FieldLabel } from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { useOAuthProvidersQuery } from '@/lib/api-vue-query';

const props = withDefaults(defineProps<{ redirectAfter?: string; errorRedirectAfter?: string }>(), {
  redirectAfter: '/',
  errorRedirectAfter: '/',
});

const { data } = useOAuthProvidersQuery();
const apiBase = import.meta.env.VITE_API_BASE ?? '/api';
const instanceUrls = ref<Record<string, string>>({});
const showOAuthError = ref(false);

onMounted(() => {
  // OAuth コールバックが失敗すると backend が ?oauth_error= 付きで戻す。
  const params = new URLSearchParams(window.location.search);
  showOAuthError.value = params.has('oauth_error');
});

const PROVIDER_LABELS: Record<string, string> = {
  github: 'GitHub',
  gitlab: 'GitLab',
  gitlab_selfhosted: 'GitLab (セルフホスト)',
  google: 'Google',
  oidc: 'OIDC',
};

function providerLabel(provider: string): string {
  return PROVIDER_LABELS[provider] ?? provider;
}

function startOAuth(provider: string, requiresInstanceUrl: boolean) {
  const params = new URLSearchParams();
  params.set('redirect_after', props.redirectAfter);
  // プロバイダーエラー時は OAuth ボタンのあるページ（signin/signup）へ戻してエラーを表示させる。
  params.set('error_redirect_after', props.errorRedirectAfter);
  if (requiresInstanceUrl) {
    const instanceUrl = instanceUrls.value[provider]?.trim();
    if (!instanceUrl) return;
    params.set('instance_url', instanceUrl);
  }
  // openapi-fetch クライアントは 302 をパースできないため、必ずフルページ遷移させる。
  window.location.assign(`${apiBase}/v1/auth/oauth/${provider}?${params.toString()}`);
}
</script>

<template>
  <div v-if="data && data.providers.length > 0" class="flex flex-col gap-3">
    <div class="flex items-center gap-3">
      <span class="bg-border h-px flex-1" />
      <span class="text-muted-foreground text-xs">または</span>
      <span class="bg-border h-px flex-1" />
    </div>
    <p v-if="showOAuthError" class="text-destructive text-center text-sm">
      外部プロバイダーでの認証に失敗しました。もう一度お試しください。
    </p>
    <template v-for="provider in data.providers" :key="provider.provider">
      <div v-if="provider.requires_instance_url" class="flex flex-col gap-2">
        <Field>
          <FieldLabel :for="`oauth-instance-${provider.provider}`">
            {{ providerLabel(provider.provider) }} インスタンス URL
          </FieldLabel>
          <Input
            :id="`oauth-instance-${provider.provider}`"
            type="url"
            inputmode="url"
            placeholder="https://gitlab.example.com"
            :model-value="instanceUrls[provider.provider] ?? ''"
            @update:model-value="(v) => (instanceUrls[provider.provider] = String(v))"
          />
        </Field>
        <Button
          type="button"
          variant="outline"
          class="w-full"
          :disabled="!instanceUrls[provider.provider]?.trim()"
          @click="startOAuth(provider.provider, true)"
        >
          {{ providerLabel(provider.provider) }} で続ける
        </Button>
      </div>
      <Button
        v-else
        type="button"
        variant="outline"
        class="w-full"
        @click="startOAuth(provider.provider, false)"
      >
        {{ providerLabel(provider.provider) }} で続ける
      </Button>
    </template>
  </div>
</template>
