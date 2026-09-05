<script setup lang="ts">
import { useQueryClient } from '@tanstack/vue-query';
import {
  PhCheckCircle,
  PhGithubLogo,
  PhGitlabLogoSimple,
  PhGoogleLogo,
  PhInfo,
  PhKey,
  PhPlus,
  PhShieldCheck,
  PhWarningCircle,
  PhX,
} from '@phosphor-icons/vue';
import { computed, onMounted, ref, type Component } from 'vue';
import { Button } from '@/components/ui/button';
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { Skeleton } from '@/components/ui/skeleton';
import PasswordMethodRow from '@/components/settings/PasswordMethodRow.vue';
import {
  meQueryOptions,
  oauthConnectionsQueryOptions,
  useDisconnectOAuthMutation,
  useOAuthConnectionsQuery,
  useOAuthProvidersQuery,
  usePasskeysQuery,
} from '@/lib/api-vue-query';
import { countAuthMethods, formatConnectedAt, type OAuthConnection } from '@/lib/auth-methods';
import { isKnownProvider, providerLabel, startOAuth } from '@/lib/oauth-providers';
import type { components } from '@/generated/api';

const props = defineProps<{ user: components['schemas']['UserResponse'] }>();

/** この画面へ戻す。連携直後だけ `linked` を付けて、戻ったことを伝える。 */
const SECURITY_PATH = '/settings/security';

const queryClient = useQueryClient();
const connectionsQuery = useOAuthConnectionsQuery();
const providersQuery = useOAuthProvidersQuery();
const passkeysQuery = usePasskeysQuery();
const disconnect = useDisconnectOAuthMutation();

const flash = ref<string | null>(null);
const oauthFailed = ref(false);
const confirmingKey = ref<string | null>(null);
const rowError = ref<Record<string, string>>({});
const instanceDrafts = ref<Record<string, string>>({});

onMounted(() => {
  const params = new URLSearchParams(window.location.search);
  // 知らない値は無視する。URL から来た文字列をそのまま通知文へ入れると、
  // その画面を開かせるだけで任意の文面を「設定画面が出した通知」として読ませられる
  const linked = params.get('linked');
  const linkedProvider = linked && isKnownProvider(linked) ? linked : null;
  if (linkedProvider) flash.value = `${providerLabel(linkedProvider)} を連携しました。`;
  // コールバックが失敗すると backend が ?oauth_error= を付けてここへ戻す。
  oauthFailed.value = params.has('oauth_error');
  if (linked || oauthFailed.value) {
    // 再読み込みで同じ通知が出ないよう、印だけ URL から落とす。
    // state は引き継ぐ（null を渡すと vike のクライアントルーターの state を捨てる）
    window.history.replaceState(window.history.state, '', SECURITY_PATH);
  }
});

const connections = computed<OAuthConnection[]>(
  () => connectionsQuery.data.value?.connections ?? [],
);
const providers = computed(() => providersQuery.data.value?.providers ?? []);
const passkeyCount = computed(() => passkeysQuery.data.value?.passkeys?.length ?? 0);

const methodCount = computed(() =>
  countAuthMethods({
    hasPassword: props.user.has_password,
    connectionCount: connections.value.length,
    passkeyCount: passkeyCount.value,
  }),
);

/** 同じプロバイダーでもインスタンスが違えば別の連携なので、両方で1件を指す。 */
function connectionKey(connection: OAuthConnection): string {
  return `${connection.provider}:${connection.instance_url ?? ''}`;
}

const linkedProviders = computed(() =>
  connections.value.map((connection) => ({
    key: connectionKey(connection),
    connection,
    label: providerLabel(connection.provider),
  })),
);

/** 未連携のプロバイダーだけ「追加できる連携」に出す。 */
const availableProviders = computed(() => {
  const linked = new Set(connections.value.map((connection) => connection.provider));
  return providers.value.filter((provider) => !linked.has(provider.provider));
});

const PROVIDER_ICONS: Record<string, Component> = {
  github: PhGithubLogo,
  gitlab: PhGitlabLogoSimple,
  gitlab_selfhosted: PhGitlabLogoSimple,
  google: PhGoogleLogo,
  oidc: PhShieldCheck,
};

function providerIcon(provider: string): Component {
  return PROVIDER_ICONS[provider] ?? PhKey;
}

function providerHint(provider: components['schemas']['OAuthProviderItem']): string {
  return provider.requires_instance_url
    ? 'インスタンス URL を指定して連携します'
    : `${providerLabel(provider.provider)} アカウントでサインインできるようにします`;
}

function onLink(provider: components['schemas']['OAuthProviderItem']) {
  const instanceUrl = provider.requires_instance_url
    ? instanceDrafts.value[provider.provider]?.trim()
    : undefined;
  if (provider.requires_instance_url && !instanceUrl) return;

  startOAuth(provider.provider, {
    redirectAfter: `${SECURITY_PATH}?linked=${encodeURIComponent(provider.provider)}`,
    errorRedirectAfter: SECURITY_PATH,
    instanceUrl,
  });
}

function askUnlink(key: string) {
  confirmingKey.value = key;
  delete rowError.value[key];
}

function messageOf(e: unknown): string | undefined {
  return (e as { error?: { message?: string } }).error?.message;
}

async function onUnlink(connection: OAuthConnection) {
  const key = connectionKey(connection);
  confirmingKey.value = null;
  delete rowError.value[key];

  try {
    await disconnect.mutateAsync({
      params: {
        path: { provider: connection.provider },
        ...(connection.instance_url ? { query: { instance_url: connection.instance_url } } : {}),
      },
      parseAs: 'text',
    });
    flash.value = `${providerLabel(connection.provider)} の連携を解除しました。`;
    await queryClient.invalidateQueries({ queryKey: oauthConnectionsQueryOptions().queryKey });
  } catch (e) {
    rowError.value[key] =
      messageOf(e) === 'oauth-last-auth-method'
        ? 'これが最後の認証方法のため解除できません。先にパスワードを設定するか、別のプロバイダーを追加してください。'
        : '解除に失敗しました。時間をおいて再度お試しください。';
  }
}

async function onPasswordSet() {
  flash.value = 'パスワードを設定しました。';
  await queryClient.invalidateQueries({ queryKey: meQueryOptions().queryKey });
}
</script>

<template>
  <div class="flex flex-col gap-4">
    <div class="flex flex-col gap-1">
      <h3 class="text-base font-semibold">認証方法</h3>
      <p class="text-muted-foreground text-sm">
        Task にサインインできる方法です。少なくとも1つは残しておく必要があります。
      </p>
    </div>

    <div
      v-if="flash"
      class="flex items-center gap-2 rounded-lg border border-green-600/40 bg-green-600/5 p-3"
    >
      <PhCheckCircle class="size-4 shrink-0 text-green-700 dark:text-green-500" />
      <p class="flex-1 text-sm">{{ flash }}</p>
      <Button
        type="button"
        variant="ghost"
        size="icon-sm"
        aria-label="通知を閉じる"
        @click="flash = null"
      >
        <PhX class="size-4" />
      </Button>
    </div>

    <p v-if="oauthFailed" role="alert" class="text-destructive text-sm">
      外部プロバイダーでの連携に失敗しました。もう一度お試しください。
    </p>

    <div class="divide-y overflow-hidden rounded-lg border">
      <PasswordMethodRow
        :has-password="user.has_password"
        :email="user.email"
        @set="onPasswordSet"
      />

      <div v-if="connectionsQuery.isLoading.value" class="flex flex-col gap-2 p-4">
        <Skeleton class="h-9 w-full" />
      </div>

      <p
        v-else-if="connectionsQuery.isError.value"
        role="alert"
        class="text-destructive p-4 text-sm"
      >
        連携済みのプロバイダーを取得できませんでした。
      </p>

      <div v-for="item in linkedProviders" :key="item.key" class="flex flex-col">
        <div class="flex flex-wrap items-center gap-3 p-4">
          <span class="bg-secondary flex size-9 shrink-0 items-center justify-center rounded-lg">
            <component :is="providerIcon(item.connection.provider)" class="size-5" />
          </span>
          <div class="min-w-52 flex-1 overflow-hidden">
            <div class="flex flex-wrap items-center gap-2">
              <span class="text-sm font-medium">{{ item.label }}</span>
              <span class="text-muted-foreground font-mono text-xs">{{
                item.connection.provider
              }}</span>
            </div>
            <p v-if="item.connection.provider_email" class="text-muted-foreground text-xs">
              {{ item.connection.provider_email }}
            </p>
            <p class="text-muted-foreground text-xs">
              接続日時 {{ formatConnectedAt(item.connection.connected_at) }}
            </p>
            <p
              v-if="item.connection.instance_url"
              class="text-muted-foreground font-mono text-xs break-all"
            >
              {{ item.connection.instance_url }}
            </p>
            <p
              v-if="methodCount <= 1"
              class="text-muted-foreground flex items-center gap-1 text-xs"
            >
              <PhInfo class="size-3.5 shrink-0" />
              これが最後の認証方法の可能性があります。解除するとサインインできなくなります。
            </p>
          </div>
          <Button
            type="button"
            variant="outline"
            size="sm"
            class="text-destructive"
            @click="askUnlink(item.key)"
          >
            解除
          </Button>
        </div>

        <div v-if="confirmingKey === item.key" class="px-4 pb-4 md:pl-16">
          <div class="flex flex-wrap items-center gap-3 rounded-md border p-3">
            <p class="min-w-52 flex-1 text-sm">
              <strong class="font-semibold">{{ item.label }}</strong>
              の連携を解除しますか？ {{ item.label }} でサインインできなくなります。
            </p>
            <Button
              type="button"
              variant="destructive"
              size="sm"
              :disabled="disconnect.isPending.value"
              @click="onUnlink(item.connection)"
            >
              解除する
            </Button>
            <Button type="button" variant="outline" size="sm" @click="confirmingKey = null">
              キャンセル
            </Button>
          </div>
        </div>

        <div v-if="rowError[item.key]" class="px-4 pb-4 md:pl-16">
          <div class="border-destructive flex items-start gap-2 rounded-md border p-3">
            <PhWarningCircle class="text-destructive mt-0.5 size-4 shrink-0" />
            <p role="alert" class="flex-1 text-sm">{{ rowError[item.key] }}</p>
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              aria-label="エラーを閉じる"
              @click="delete rowError[item.key]"
            >
              <PhX class="size-4" />
            </Button>
          </div>
        </div>
      </div>

      <template v-if="availableProviders.length > 0">
        <p class="bg-secondary text-muted-foreground px-4 py-3 text-xs font-medium">
          追加できる連携
        </p>
        <div v-for="provider in availableProviders" :key="provider.provider" class="flex flex-col">
          <div class="flex flex-wrap items-center gap-3 p-4">
            <span class="bg-secondary flex size-9 shrink-0 items-center justify-center rounded-lg">
              <component :is="providerIcon(provider.provider)" class="size-5" />
            </span>
            <div class="min-w-48 flex-1">
              <div class="flex flex-wrap items-center gap-2">
                <span class="text-sm font-medium">{{ providerLabel(provider.provider) }}</span>
                <span class="text-muted-foreground font-mono text-xs">{{ provider.provider }}</span>
              </div>
              <p class="text-muted-foreground text-xs">{{ providerHint(provider) }}</p>
            </div>
            <Button
              type="button"
              variant="outline"
              size="sm"
              :disabled="
                provider.requires_instance_url && !instanceDrafts[provider.provider]?.trim()
              "
              @click="onLink(provider)"
            >
              <PhPlus class="size-4" />
              連携する
            </Button>
          </div>

          <div v-if="provider.requires_instance_url" class="px-4 pb-4 md:pl-16">
            <Field>
              <FieldLabel :for="`instance-${provider.provider}`">インスタンス URL</FieldLabel>
              <Input
                :id="`instance-${provider.provider}`"
                type="url"
                inputmode="url"
                placeholder="https://gitlab.example.com"
                class="max-w-md font-mono"
                :model-value="instanceDrafts[provider.provider] ?? ''"
                @update:model-value="(v) => (instanceDrafts[provider.provider] = String(v))"
              />
              <FieldDescription>
                インスタンス URL の入力が必要なプロバイダーです。承認前に指定してください。
              </FieldDescription>
            </Field>
          </div>
        </div>
      </template>
    </div>
  </div>
</template>
