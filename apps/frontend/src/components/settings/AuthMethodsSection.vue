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
import { computed, onMounted, ref, watch, type Component } from 'vue';
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
import { consumeNotice, markNotice, OAUTH_LINK_NOTICE } from '@/lib/one-time-notice';
import { isKnownProvider, providerLabel, startOAuth } from '@/lib/oauth-providers';
import type { components } from '@/generated/api';

const props = defineProps<{ user: components['schemas']['UserResponse'] }>();

/** 承認のあとこの画面へ戻す。 */
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

/** 連携を開始したプロバイダー。連携一覧に現れるまで通知を保留する。 */
const pendingLinked = ref<string | null>(null);

onMounted(() => {
  const params = new URLSearchParams(window.location.search);
  // コールバックが失敗すると backend が ?oauth_error= を付けてここへ戻す。
  oauthFailed.value = params.has('oauth_error');
  // 通知の根拠は URL ではなく、連携を始めたときにこのタブへ置いた印。
  const started = consumeNotice(OAUTH_LINK_NOTICE);
  if (!oauthFailed.value && started && isKnownProvider(started)) pendingLinked.value = started;
  if (oauthFailed.value) {
    // 再読み込みで同じ通知が出ないよう、印だけ URL から落とす。
    // state は引き継ぐ（null を渡すと vike のクライアントルーターの state を捨てる）
    window.history.replaceState(window.history.state, '', SECURITY_PATH);
  }
});

const connections = computed<OAuthConnection[]>(
  () => connectionsQuery.data.value?.connections ?? [],
);
// 実際に連携一覧へ入ったことまで確かめてから通知する。開始の印だけを見ると、
// 承認の途中で失敗してこの画面に戻らなかったとき、次にここを開いた時点で誤って出る
watch(
  [pendingLinked, connections],
  () => {
    const provider = pendingLinked.value;
    if (!provider) return;
    if (!connections.value.some((connection) => connection.provider === provider)) return;
    flash.value = `${providerLabel(provider)} を連携しました。`;
    pendingLinked.value = null;
  },
  { immediate: true },
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

/**
 * 「追加できる連携」に出すプロバイダー。
 *
 * インスタンス URL を取るプロバイダー（GitLab セルフホスト）は、1 件連携済みでも候補に残す。
 * backend は (provider, provider_user_id, instance_url) で接続を識別していて、インスタンスが
 * 違えば別の連携として足せる。プロバイダー名だけで消すと、2 つ目のインスタンスを画面から
 * 追加できなくなる。
 */
const availableProviders = computed(() => {
  const linked = new Set(connections.value.map((connection) => connection.provider));
  return providers.value.filter(
    (provider) => provider.requires_instance_url || !linked.has(provider.provider),
  );
});

/**
 * 同じインスタンスを指しているかの判定用。
 *
 * backend の正規化（`normalize_instance_url`）と厳密に揃える必要はない。ここで通しても
 * 重複は backend が 409 で弾く。逆に別のインスタンスを取り違えて止めないよう、URL として
 * 解釈できたときだけ origin + パスで比べる。
 */
function normalizeInstance(url: string): string {
  const trimmed = url.trim();
  try {
    const parsed = new URL(trimmed);
    return `${parsed.origin}${parsed.pathname.replace(/\/+$/, '')}`;
  } catch {
    return trimmed.replace(/\/+$/, '');
  }
}

/** プロバイダーごとの、連携済みインスタンス。 */
const linkedInstances = computed(() => {
  const byProvider = new Map<string, string[]>();
  for (const connection of connections.value) {
    if (!connection.instance_url) continue;
    const list = byProvider.get(connection.provider) ?? [];
    list.push(normalizeInstance(connection.instance_url));
    byProvider.set(connection.provider, list);
  }
  return byProvider;
});

/** 入力中のインスタンスが既に連携済みか。承認画面へ飛ばす前に気づけるようにする。 */
function instanceAlreadyLinked(provider: string): boolean {
  const draft = instanceDrafts.value[provider]?.trim();
  if (!draft) return false;
  return (linkedInstances.value.get(provider) ?? []).includes(normalizeInstance(draft));
}

const PROVIDER_ICONS: Record<string, Component> = {
  github: PhGithubLogo,
  gitlab: PhGitlabLogoSimple,
  gitlab_selfhosted: PhGitlabLogoSimple,
  google: PhGoogleLogo,
  oidc: PhShieldCheck,
};

function providerIcon(provider: string): Component {
  return Object.hasOwn(PROVIDER_ICONS, provider) ? PROVIDER_ICONS[provider] : PhKey;
}

function providerHint(provider: components['schemas']['OAuthProviderItem']): string {
  if (!provider.requires_instance_url) {
    return `${providerLabel(provider.provider)} アカウントでサインインできるようにします`;
  }
  return linkedInstances.value.has(provider.provider)
    ? '別のインスタンス URL を指定して、もう1つ連携できます'
    : 'インスタンス URL を指定して連携します';
}

function onLink(provider: components['schemas']['OAuthProviderItem']) {
  const instanceUrl = provider.requires_instance_url
    ? instanceDrafts.value[provider.provider]?.trim()
    : undefined;
  if (provider.requires_instance_url && (!instanceUrl || instanceAlreadyLinked(provider.provider)))
    return;

  // 戻ってきたときの通知はこの印だけを根拠にする（URL には何も足さない）
  markNotice(OAUTH_LINK_NOTICE, provider.provider);
  startOAuth(provider.provider, {
    redirectAfter: SECURITY_PATH,
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
                provider.requires_instance_url &&
                (!instanceDrafts[provider.provider]?.trim() ||
                  instanceAlreadyLinked(provider.provider))
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
              <FieldDescription v-if="instanceAlreadyLinked(provider.provider)" role="alert">
                このインスタンスは連携済みです。別のインスタンス URL を入力してください。
              </FieldDescription>
              <FieldDescription v-else>
                インスタンス URL の入力が必要なプロバイダーです。承認前に指定してください。
              </FieldDescription>
            </Field>
          </div>
        </div>
      </template>
    </div>
  </div>
</template>
