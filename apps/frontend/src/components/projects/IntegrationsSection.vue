<script setup lang="ts">
import { useQueryClient } from '@tanstack/vue-query';
import { PhGithubLogo } from '@phosphor-icons/vue';
import { computed, onMounted, ref } from 'vue';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { apiClient, fetchClient } from '@/lib/api-vue-query';

const GITHUB_INTEGRATION_PATH =
  '/v1/tenants/{tenant_id}/projects/{project_id}/github/integration' as const;
const GITHUB_INSTALL_PATH = '/v1/tenants/{tenant_id}/projects/{project_id}/github/install' as const;
const GITHUB_REPOSITORIES_PATH =
  '/v1/tenants/{tenant_id}/projects/{project_id}/github/repositories' as const;
const GITHUB_CONNECT_PATH = '/v1/tenants/{tenant_id}/projects/{project_id}/github/connect' as const;

const props = defineProps<{
  tenantId: string;
  projectId: string;
}>();

const queryClient = useQueryClient();

const CALLBACK_ERRORS: Record<string, string> = {
  no_repositories:
    'インストールにリポジトリが 1 件も含まれていません。GitHub 側でリポジトリを追加してから、もう一度お試しください。',
  installation_rejected:
    'このインストールでは連携できませんでした。GitHub の設定から一度アンインストールしてから、もう一度お試しください。',
  installation_forbidden:
    'このインストールはあなたのアカウントからは操作できません。ご自身がアクセスできるアカウントまたは Organization に、もう一度インストールしてください。',
  installation_authorization_required:
    'GitHub App の設定でユーザー認可が有効になっていないため、連携できませんでした。管理者に設定の確認を依頼してください。',
  github_unavailable: 'GitHub と通信できませんでした。時間をおいて、もう一度お試しください。',
};

/** 設定セクションを切り替えるとこのコンポーネントは破棄されるので、
 * URL から落とした選択トークンはタブ内に退避しておく（TTL は 10 分）。 */
const SELECT_TOKEN_STORAGE_KEY = 'github-select-token';

// callback が付けた値は、読んだらすぐ URL から落とす（下の clearCallbackQuery）。
// 選択トークンはフラグメントで渡ってくる（クエリだと frontend / CDN のアクセスログと
// Referer に残るため）。pageContext は replaceState もフラグメントも反映しないので、
// window.location から読む。
const selectToken = ref<string | null>(null);
const repositories = ref<{ owner: string; name: string }[]>([]);
const repositoryFilter = ref('');
const callbackError = ref<string | null>(null);
const selectError = ref<string | null>(null);
const selectPending = ref(false);
const isDisconnectOpen = ref(false);
const disconnectError = ref<string | null>(null);
const installError = ref<string | null>(null);
const installPending = ref(false);

const integrationQuery = apiClient.useQuery('get', GITHUB_INTEGRATION_PATH, {
  params: { path: { tenant_id: props.tenantId, project_id: props.projectId } },
});

const disconnectMutation = apiClient.useMutation('delete', GITHUB_INTEGRATION_PATH);

const integration = computed(() => integrationQuery.data.value);

const repoFullName = computed(() => {
  const data = integration.value;
  if (!data?.connected || !data.repo_owner || !data.repo_name) return null;
  return `${data.repo_owner}/${data.repo_name}`;
});

const connectedAtLabel = computed(() => {
  const at = integration.value?.connected_at;
  if (!at) return null;
  return new Date(at).toLocaleDateString('ja-JP', {
    year: 'numeric',
    month: 'long',
    day: 'numeric',
  });
});

/** 選択トークン自体が無効になったことを示すステータスか（それ以外は一時障害扱い） */
function isSelectTokenDead(status: number) {
  return status === 400 || status === 403;
}

/** callback が付けた値を URL から落とす（トークンを履歴に残さない、
 * リロードでエラーが蘇らない） */
function clearCallbackQuery() {
  const url = new URL(window.location.href);
  url.searchParams.delete('github_error');
  // backend はトークンを単独のフラグメント（`#github_select=...`）で返すので、
  // それが載っているときだけ触る（他の断片との同居は考慮していない）。
  const hash = new URLSearchParams(url.hash.slice(1));
  if (hash.has('github_select')) {
    hash.delete('github_select');
    const rest = hash.toString();
    url.hash = rest ? `#${rest}` : '';
  }
  window.history.replaceState(window.history.state, '', url);
}

/** 選択トークンが切れていたら選択 UI を畳んで未連携表示に戻す */
async function loadRepositories() {
  const token = selectToken.value;
  if (!token) return;
  selectError.value = null;
  selectPending.value = true;
  try {
    // トークンはクエリではなくヘッダーに載せる。クエリだと backend とその手前の
    // プロキシのアクセスログに残り、フラグメントで渡した手当てが台無しになる。
    const { data, error, response } = await fetchClient.GET(GITHUB_REPOSITORIES_PATH, {
      params: {
        path: { tenant_id: props.tenantId, project_id: props.projectId },
        header: { 'X-Github-Select-Token': token },
      },
    });
    if (error || !data) {
      // 4xx はトークンが無効（期限切れ・使用済み）。それ以外は一時障害なので
      // トークンを捨てず、再試行させる。
      // トークンが無効なのは 400 / 403 のときだけ。401（セッション切れ）や
      // 5xx でトークンを捨てると、まだ使えるのにやり直しになる。
      if (isSelectTokenDead(response.status)) {
        forgetSelectToken();
        selectError.value = '選択の有効期限が切れました。もう一度「連携する」を押してください。';
        return;
      }
      throw new Error('repositories-unavailable');
    }
    repositories.value = data.repositories;
  } catch {
    selectError.value = 'リポジトリ一覧を取得できませんでした';
  } finally {
    selectPending.value = false;
  }
}

function stashKey() {
  return `${SELECT_TOKEN_STORAGE_KEY}:${props.projectId}`;
}

function forgetSelectToken() {
  selectToken.value = null;
  repositories.value = [];
  repositoryFilter.value = '';
  window.sessionStorage.removeItem(stashKey());
}

onMounted(() => {
  const search = new URLSearchParams(window.location.search);
  const fromUrl = new URLSearchParams(window.location.hash.slice(1)).get('github_select');
  selectToken.value = fromUrl ?? window.sessionStorage.getItem(stashKey());
  if (fromUrl) window.sessionStorage.setItem(stashKey(), fromUrl);
  callbackError.value = CALLBACK_ERRORS[search.get('github_error') ?? ''] ?? null;
  clearCallbackQuery();
  void loadRepositories();
});

/** 数百リポジトリの org でも目的のものへ辿り着けるよう、owner/name の部分一致で絞る。 */
const filteredRepositories = computed(() => {
  const keyword = repositoryFilter.value.trim().toLowerCase();
  if (!keyword) return repositories.value;
  return repositories.value.filter((repo) =>
    `${repo.owner}/${repo.name}`.toLowerCase().includes(keyword),
  );
});

async function connectRepository(owner: string, name: string) {
  const token = selectToken.value;
  if (!token) return;
  selectError.value = null;
  selectPending.value = true;
  try {
    const { error, response } = await fetchClient.POST(GITHUB_CONNECT_PATH, {
      params: { path: { tenant_id: props.tenantId, project_id: props.projectId } },
      body: { select_token: token, repo_owner: owner, repo_name: name },
    });
    if (error) {
      if (isSelectTokenDead(response.status)) {
        // トークン切れか、その間にリポジトリが外れたか。一覧を取り直せばどちらか分かる
        // （トークンが死んでいれば loadRepositories が期限切れとして畳む）。
        await loadRepositories();
        if (selectToken.value) {
          selectError.value = 'このリポジトリは選べませんでした。別のものを選んでください。';
        }
        return;
      }
      throw new Error('connect-failed');
    }
    forgetSelectToken();
    await queryClient.invalidateQueries({ queryKey: ['get', GITHUB_INTEGRATION_PATH] });
  } catch {
    selectError.value = 'リポジトリを連携できませんでした';
  } finally {
    selectPending.value = false;
  }
}

async function startInstall() {
  installError.value = null;
  installPending.value = true;
  try {
    const { data, error } = await fetchClient.GET(GITHUB_INSTALL_PATH, {
      params: { path: { tenant_id: props.tenantId, project_id: props.projectId } },
    });
    if (error || !data) throw new Error('install-url-unavailable');
    // GitHub App のインストール画面（外部 URL）へ遷移する
    window.location.assign(data.url);
  } catch {
    installError.value = 'GitHub のインストール URL を取得できませんでした';
    installPending.value = false;
  }
}

function onDisconnectOpenChange(open: boolean) {
  // 解除リクエスト進行中はダイアログを閉じない（結果の見逃し防止）
  if (!open && disconnectMutation.isPending.value) return;
  if (open) disconnectError.value = null;
  isDisconnectOpen.value = open;
}

async function confirmDisconnect() {
  disconnectError.value = null;
  try {
    await disconnectMutation.mutateAsync({
      params: { path: { tenant_id: props.tenantId, project_id: props.projectId } },
    });
    await queryClient.invalidateQueries({ queryKey: ['get', GITHUB_INTEGRATION_PATH] });
    isDisconnectOpen.value = false;
  } catch {
    disconnectError.value = '連携を解除できませんでした';
  }
}
</script>

<template>
  <div>
    <h2 class="mb-6 border-b pb-4 text-xl font-semibold">連携</h2>

    <p v-if="integrationQuery.isPending.value" role="status" class="text-sm text-muted-foreground">
      連携状態を読み込み中…
    </p>

    <div v-else-if="integrationQuery.isError.value" class="flex items-center gap-3">
      <p role="alert" class="text-sm text-destructive">連携状態を取得できませんでした</p>
      <Button type="button" variant="outline" size="sm" @click="() => integrationQuery.refetch()">
        再試行
      </Button>
    </div>

    <div v-else class="flex flex-col gap-3">
      <!-- GitHub カード（Slack / Figma は API 実装後に追加） -->
      <div class="flex items-center gap-3.5 rounded-[10px] border p-4">
        <span
          class="flex size-10 shrink-0 items-center justify-center rounded-lg bg-secondary"
          aria-hidden="true"
        >
          <PhGithubLogo class="size-5" />
        </span>
        <div class="min-w-0 flex-1">
          <p class="text-sm font-medium">GitHub</p>
          <p class="mt-0.5 truncate text-xs text-muted-foreground">
            <template v-if="repoFullName">
              <span class="font-mono">{{ repoFullName }}</span> を連携中<template
                v-if="connectedAtLabel"
                >（{{ connectedAtLabel }} から）</template
              >
            </template>
            <template v-else>コミットや Pull Request をタスクに紐付けます。</template>
          </p>
        </div>
        <Button
          v-if="integration?.connected"
          type="button"
          variant="outline"
          size="sm"
          class="shrink-0"
          @click="onDisconnectOpenChange(true)"
        >
          連携を解除
        </Button>
        <Button
          v-else
          type="button"
          size="sm"
          class="shrink-0"
          :disabled="installPending"
          @click="startInstall"
        >
          {{ installPending ? '接続中…' : '連携する' }}
        </Button>
      </div>
      <p v-if="installError" role="alert" class="text-sm text-destructive">{{ installError }}</p>
      <p v-if="callbackError" role="alert" class="text-sm text-destructive">{{ callbackError }}</p>

      <!-- インストールに複数リポジトリが含まれるとき、連携先を 1 件選ばせる -->
      <div v-if="selectToken || selectError" class="rounded-[10px] border p-4">
        <p class="text-sm font-medium">連携するリポジトリを選択</p>
        <p class="mt-0.5 text-xs text-muted-foreground">
          このインストールから 1 つのリポジトリをプロジェクトに紐付けます。
        </p>
        <p
          v-if="selectToken && !selectError && !repositories.length"
          :role="selectPending ? 'status' : 'alert'"
          class="mt-3 text-sm text-muted-foreground"
        >
          {{ selectPending ? 'リポジトリを読み込み中…' : '選択できるリポジトリがありません' }}
        </p>
        <template v-else-if="repositories.length">
          <!-- 数百リポジトリの org では全件を並べても選べないので、手元で絞り込む -->
          <Input
            v-model="repositoryFilter"
            class="mt-3"
            type="search"
            aria-label="リポジトリを絞り込む"
            placeholder="owner/name で絞り込む"
          />
          <p
            v-if="!filteredRepositories.length"
            role="status"
            class="mt-3 text-sm text-muted-foreground"
          >
            「{{ repositoryFilter }}」に一致するリポジトリはありません
          </p>
          <ul v-else class="mt-3 flex max-h-72 flex-col gap-1.5 overflow-y-auto">
            <li
              v-for="repo in filteredRepositories"
              :key="`${repo.owner}/${repo.name}`"
              class="flex items-center gap-3 rounded-md border p-2.5"
            >
              <span class="min-w-0 flex-1 truncate font-mono text-sm"
                >{{ repo.owner }}/{{ repo.name }}</span
              >
              <Button
                type="button"
                size="sm"
                variant="outline"
                class="shrink-0"
                :disabled="selectPending"
                @click="connectRepository(repo.owner, repo.name)"
              >
                選択
              </Button>
            </li>
          </ul>
        </template>
        <div v-if="selectError" class="mt-3 flex items-center gap-3">
          <p role="alert" class="text-sm text-destructive">{{ selectError }}</p>
          <Button
            v-if="selectToken && !repositories.length"
            type="button"
            variant="outline"
            size="sm"
            :disabled="selectPending"
            @click="loadRepositories"
          >
            再試行
          </Button>
        </div>
      </div>
    </div>

    <Dialog v-if="isDisconnectOpen" :open="true" @update:open="onDisconnectOpenChange">
      <DialogContent class="max-w-md" :show-close-button="false">
        <DialogHeader>
          <DialogTitle>GitHub 連携を解除しますか？</DialogTitle>
          <DialogDescription>
            <template v-if="repoFullName">「{{ repoFullName }}」との連携を解除します。</template>
            <template v-else>GitHub との連携を解除します。</template>
            コミットや Pull Request の紐付けは更新されなくなります。
          </DialogDescription>
        </DialogHeader>
        <p v-if="disconnectError" role="alert" class="text-sm text-destructive">
          {{ disconnectError }}
        </p>
        <DialogFooter>
          <Button
            type="button"
            variant="outline"
            :disabled="disconnectMutation.isPending.value"
            @click="onDisconnectOpenChange(false)"
          >
            キャンセル
          </Button>
          <Button
            type="button"
            variant="destructive"
            :disabled="disconnectMutation.isPending.value"
            @click="confirmDisconnect"
          >
            {{ disconnectMutation.isPending.value ? '解除中…' : '解除する' }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>
