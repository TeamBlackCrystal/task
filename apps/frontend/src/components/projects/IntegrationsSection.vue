<script setup lang="ts">
import { useQueryClient } from '@tanstack/vue-query';
import { PhGithubLogo } from '@phosphor-icons/vue';
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import { Button } from '@/components/ui/button';
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
const GITHUB_IMPORT_PATH = '/v1/tenants/{tenant_id}/projects/{project_id}/github/import' as const;

/** 取り込み開始が成功したあと、再度押せるようになるまでの待ち時間（ミリ秒） */
const IMPORT_COOLDOWN_MS = 60_000;

const props = defineProps<{
  tenantId: string;
  projectId: string;
}>();

const queryClient = useQueryClient();
const isDisconnectOpen = ref(false);
const disconnectError = ref<string | null>(null);
const installError = ref<string | null>(null);
const installPending = ref(false);
const importError = ref<string | null>(null);
const importStarted = ref(false);
const importCoolingDown = ref(false);
let importCooldownTimer: ReturnType<typeof setTimeout> | null = null;

const integrationQuery = apiClient.useQuery('get', GITHUB_INTEGRATION_PATH, {
  params: { path: { tenant_id: props.tenantId, project_id: props.projectId } },
});

const disconnectMutation = apiClient.useMutation('delete', GITHUB_INTEGRATION_PATH);
const importMutation = apiClient.useMutation('post', GITHUB_IMPORT_PATH);

const integration = computed(() => integrationQuery.data.value);

const importDisabled = computed(() => importMutation.isPending.value || importCoolingDown.value);

const importLabel = computed(() => {
  if (importMutation.isPending.value) return '開始中…';
  if (importCoolingDown.value) return '取り込み中…';
  return 'Issue を取り込む';
});

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

function clearImportCooldown() {
  if (importCooldownTimer !== null) {
    clearTimeout(importCooldownTimer);
    importCooldownTimer = null;
  }
  importCoolingDown.value = false;
}

// 202 は「ジョブを積んだ」だけなので、連打すると同じ全 Issue クロールが
// その回数だけ積まれる。成功後は一定時間ボタンを塞ぐ
function startImportCooldown() {
  clearImportCooldown();
  importCoolingDown.value = true;
  importCooldownTimer = setTimeout(() => {
    importCooldownTimer = null;
    importCoolingDown.value = false;
  }, IMPORT_COOLDOWN_MS);
}

onBeforeUnmount(clearImportCooldown);

function resetImportState() {
  importStarted.value = false;
  importError.value = null;
  clearImportCooldown();
}

// 解除は別タブ・別ユーザーからも起きるので、自分の解除操作ではなく連携状態の変化で捨てる。
// 表示を隠すだけだと、再連携で connected が true に戻った瞬間に前回の結果表示が戻る
watch(
  () => (integration.value?.connected ? repoFullName.value : null),
  () => resetImportState(),
);

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

async function startImport() {
  importError.value = null;
  importStarted.value = false;
  try {
    await importMutation.mutateAsync({
      params: { path: { tenant_id: props.tenantId, project_id: props.projectId } },
    });
    // 取り込みはジョブなので、完了は待たずに開始だけを伝える
    importStarted.value = true;
    startImportCooldown();
  } catch {
    importError.value = 'Issue の取り込みを開始できませんでした';
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
        <div v-if="integration?.connected" class="flex shrink-0 gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            :disabled="importDisabled"
            @click="startImport"
          >
            {{ importLabel }}
          </Button>
          <Button type="button" variant="outline" size="sm" @click="onDisconnectOpenChange(true)">
            連携を解除
          </Button>
        </div>
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
      <!-- 連携解除後に取り込みの結果表示が残らないよう、連携中だけ出す -->
      <p
        v-if="integration?.connected && importStarted"
        role="status"
        class="text-sm text-muted-foreground"
      >
        Issue の取り込みを開始しました。タスクに反映されるまで少し時間がかかります。
      </p>
      <p v-if="integration?.connected && importError" role="alert" class="text-sm text-destructive">
        {{ importError }}
      </p>
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
