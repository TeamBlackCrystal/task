<script setup lang="ts">
import { useQueryClient } from '@tanstack/vue-query';
import { PhCaretDown, PhCopy, PhKey, PhPlus } from '@phosphor-icons/vue';
import { computed, ref } from 'vue';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel } from '@/components/ui/field';
import HydrationSafeForm from '@/components/HydrationSafeForm.vue';
import { Input } from '@/components/ui/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Skeleton } from '@/components/ui/skeleton';
import {
  personalTokensQueryOptions,
  useCreatePersonalTokenMutation,
  usePersonalTokensQuery,
  useRevokePersonalTokenMutation,
  useTenantsQuery,
} from '@/lib/api-vue-query';
import {
  EXPIRATION_PRESETS,
  SCOPE_CATALOG,
  expiresAtFromPreset,
  formatExpiry,
  formatLastUsed,
  maskedToken,
  type ExpirationKey,
  type PersonalToken,
  type TokenScope,
} from '@/lib/personal-tokens';
import type { components } from '@/generated/api';

const props = defineProps<{ user: components['schemas']['UserResponse'] }>();

const TOKEN_NAME_MAX = 100;

const queryClient = useQueryClient();
const tokensQuery = usePersonalTokensQuery();
const tenantsQuery = useTenantsQuery();
const createToken = useCreatePersonalTokenMutation();
const revokeToken = useRevokePersonalTokenMutation();

/** PAT はテナントオーナーしか発行できないため、選択肢を自分がオーナーのテナントに絞る。 */
const ownedTenants = computed(() => {
  // operationId 重複（admin 側の list_tenants）で生成型が配列とのユニオンになるため、両形に対応する。
  const data = tenantsQuery.data.value;
  const tenants = Array.isArray(data) ? data : (data?.tenants ?? []);
  return tenants.filter((t) => t.owner_id === props.user.id);
});

// --- 発行フォーム ---

const isFormOpen = ref(false);
const tokenName = ref('');
const nameError = ref<string | null>(null);
const selectedTenantId = ref<string | null>(null);
const expiration = ref<ExpirationKey>('90d');
const selectedScopes = ref<TokenScope[]>([]);
const scopesError = ref<string | null>(null);
const submitError = ref<string | null>(null);

/** 発行直後の平文トークン。この画面でしか見られない。 */
const createdToken = ref<string | null>(null);
const copied = ref(false);

const formTenantId = computed(() => selectedTenantId.value ?? ownedTenants.value[0]?.id ?? null);

function openForm() {
  isFormOpen.value = true;
  createdToken.value = null;
  copied.value = false;
}

function resetForm() {
  isFormOpen.value = false;
  tokenName.value = '';
  nameError.value = null;
  selectedTenantId.value = null;
  expiration.value = '90d';
  selectedScopes.value = [];
  scopesError.value = null;
  submitError.value = null;
}

function toggleScope(scope: TokenScope, checked: boolean) {
  scopesError.value = null;
  selectedScopes.value = checked
    ? [...selectedScopes.value, scope]
    : selectedScopes.value.filter((s) => s !== scope);
}

function validateName(): boolean {
  const name = tokenName.value.trim();
  if (name === '') {
    nameError.value = 'トークン名を入力してください。';
    return false;
  }
  if (name.length > TOKEN_NAME_MAX) {
    nameError.value = `${TOKEN_NAME_MAX}文字以内で入力してください。`;
    return false;
  }
  nameError.value = null;
  return true;
}

async function onSubmit() {
  submitError.value = null;
  const nameOk = validateName();
  if (selectedScopes.value.length === 0) {
    scopesError.value = 'スコープを 1 つ以上選択してください。';
  }
  const tenantId = formTenantId.value;
  if (!nameOk || selectedScopes.value.length === 0 || !tenantId) return;

  try {
    const created = await createToken.mutateAsync({
      body: {
        name: tokenName.value.trim(),
        tenant_id: tenantId,
        // null = テナント内の全プロジェクトで有効。プロジェクト単位の絞り込み UI は未対応。
        project_ids: null,
        scopes: selectedScopes.value,
        expires_at: expiresAtFromPreset(expiration.value),
      },
    });
    await queryClient.invalidateQueries({ queryKey: personalTokensQueryOptions().queryKey });
    resetForm();
    createdToken.value = created.token;
  } catch (e) {
    const status = (e as { response?: { status?: number } }).response?.status;
    submitError.value =
      status === 403
        ? 'トークンを発行できるのは、選択したテナントのオーナーだけです。'
        : status === 400
          ? '入力内容を確認してください。'
          : 'トークンを発行できませんでした。時間をおいて再度お試しください。';
  }
}

async function copyCreatedToken() {
  if (!createdToken.value) return;
  await navigator.clipboard.writeText(createdToken.value);
  copied.value = true;
}

// --- 取り消し ---

const revokeTarget = ref<PersonalToken | null>(null);
const revokeError = ref<string | null>(null);

function openRevokeDialog(token: PersonalToken) {
  revokeTarget.value = token;
  revokeError.value = null;
}

function onRevokeOpenChange(open: boolean) {
  // 取り消しリクエスト進行中はダイアログを閉じない（結果の見逃し防止）
  if (!open && !revokeToken.isPending.value) revokeTarget.value = null;
}

async function onRevokeConfirm() {
  if (!revokeTarget.value) return;
  revokeError.value = null;
  try {
    await revokeToken.mutateAsync({
      params: { path: { id: revokeTarget.value.id } },
    });
    await queryClient.invalidateQueries({ queryKey: personalTokensQueryOptions().queryKey });
    revokeTarget.value = null;
  } catch {
    revokeError.value = 'トークンを取り消せませんでした。時間をおいて再度お試しください。';
  }
}
</script>

<template>
  <div class="flex flex-col gap-4">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div class="flex flex-col gap-1">
        <h2 class="text-lg font-semibold">パーソナルアクセストークン</h2>
        <p class="text-muted-foreground text-sm">
          API と CLI をあなたとして認証するトークンです。パスワードと同じように扱ってください。
        </p>
      </div>
      <Button
        v-if="!isFormOpen"
        type="button"
        :disabled="ownedTenants.length === 0"
        @click="openForm"
      >
        <PhPlus class="size-4" />
        トークンを発行
      </Button>
    </div>

    <!-- 発行直後の平文トークン。再表示できないためコピーを促す。 -->
    <div
      v-if="createdToken"
      class="flex flex-col gap-2 rounded-lg border border-green-600/40 bg-green-600/5 p-4"
    >
      <p class="text-sm font-medium">新しいトークンを発行しました。</p>
      <p class="text-muted-foreground text-sm">
        このトークンは今しか表示されません。必ずコピーして安全な場所に保管してください。
      </p>
      <div class="flex items-center gap-2">
        <code
          class="bg-muted min-w-0 flex-1 truncate rounded-md px-3 py-2 font-mono text-sm"
          data-testid="created-token"
          >{{ createdToken }}</code
        >
        <Button type="button" variant="outline" size="sm" @click="copyCreatedToken">
          <PhCopy class="size-4" />
          {{ copied ? 'コピーしました' : 'コピー' }}
        </Button>
      </div>
    </div>

    <!-- 発行フォーム -->
    <div v-if="isFormOpen" class="rounded-lg border p-4">
      <HydrationSafeForm v-slot="{ isHydrated }" @submit="onSubmit">
        <FieldGroup>
          <h3 class="text-base font-semibold">新しいトークンを発行</h3>

          <Field>
            <FieldLabel for="token-name">トークン名</FieldLabel>
            <Input
              id="token-name"
              v-model="tokenName"
              placeholder="例: CI デプロイ"
              @blur="validateName()"
              @input="nameError = null"
            />
            <FieldDescription>このトークンの用途がわかる名前を付けます。</FieldDescription>
            <FieldError class="min-h-[1.25rem]">{{ nameError ?? '' }}</FieldError>
          </Field>

          <Field v-if="ownedTenants.length > 1">
            <FieldLabel for="token-tenant">テナント</FieldLabel>
            <Select
              :model-value="formTenantId ?? undefined"
              @update:model-value="(v) => (selectedTenantId = v == null ? null : String(v))"
            >
              <SelectTrigger id="token-tenant" class="w-full">
                <SelectValue placeholder="選択してください" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem v-for="tenant in ownedTenants" :key="tenant.id" :value="tenant.id">
                  {{ tenant.name }}
                </SelectItem>
              </SelectContent>
            </Select>
            <FieldDescription>トークンはこのテナントの中でだけ使えます。</FieldDescription>
          </Field>

          <Field>
            <FieldLabel>有効期限</FieldLabel>
            <div class="flex flex-wrap gap-2">
              <Button
                v-for="preset in EXPIRATION_PRESETS"
                :key="preset.key"
                type="button"
                size="sm"
                :variant="expiration === preset.key ? 'default' : 'outline'"
                :aria-pressed="expiration === preset.key"
                @click="expiration = preset.key"
              >
                {{ preset.label }}
              </Button>
            </div>
          </Field>

          <Field>
            <FieldLabel>スコープ</FieldLabel>
            <div class="flex flex-col gap-3">
              <label
                v-for="entry in SCOPE_CATALOG"
                :key="entry.scope"
                class="flex cursor-pointer items-start gap-3"
              >
                <Checkbox
                  class="mt-0.5"
                  :model-value="selectedScopes.includes(entry.scope)"
                  :name="`scope-${entry.scope}`"
                  @update:model-value="(v) => toggleScope(entry.scope, v === true)"
                />
                <span class="flex flex-col">
                  <code class="font-mono text-sm">{{ entry.scope }}</code>
                  <span class="text-muted-foreground text-sm">{{ entry.description }}</span>
                </span>
              </label>
            </div>
            <FieldError class="min-h-[1.25rem]">{{ scopesError ?? '' }}</FieldError>
          </Field>

          <p v-if="submitError" class="text-destructive text-sm">{{ submitError }}</p>

          <Field>
            <div class="flex gap-2">
              <Button type="submit" :disabled="!isHydrated || createToken.isPending.value">
                {{ createToken.isPending.value ? '発行中…' : 'トークンを発行' }}
              </Button>
              <Button type="button" variant="outline" @click="resetForm">キャンセル</Button>
            </div>
          </Field>
        </FieldGroup>
      </HydrationSafeForm>
    </div>

    <!-- 一覧 -->
    <div v-if="tokensQuery.isPending.value" class="flex flex-col gap-2">
      <Skeleton class="h-16 w-full" />
      <Skeleton class="h-16 w-full" />
    </div>
    <p v-else-if="tokensQuery.isError.value" class="text-destructive text-sm">
      トークンの一覧を取得できませんでした。再読み込みしてください。
    </p>
    <template v-else-if="tokensQuery.isSuccess.value">
      <div
        v-if="ownedTenants.length === 0 && !tenantsQuery.isPending.value"
        class="text-muted-foreground text-sm"
      >
        トークンを発行できるのは、自分がオーナーのテナントだけです。
      </div>

      <p
        v-if="tokensQuery.data.value!.length === 0 && !isFormOpen"
        class="text-muted-foreground text-sm"
      >
        トークンはまだありません。
      </p>

      <ul
        v-else-if="tokensQuery.data.value!.length > 0"
        class="divide-y rounded-lg border"
        data-testid="token-list"
      >
        <li
          v-for="token in tokensQuery.data.value"
          :key="token.id"
          class="flex items-center gap-4 p-4"
        >
          <div
            class="bg-muted text-muted-foreground hidden size-10 shrink-0 place-content-center rounded-full sm:grid"
          >
            <PhKey class="size-5" />
          </div>
          <div class="flex min-w-0 flex-1 flex-col gap-0.5">
            <p class="truncate text-sm font-medium">{{ token.name }}</p>
            <p class="text-muted-foreground flex flex-wrap items-center gap-x-2 text-xs">
              <code class="font-mono">{{ maskedToken(token.token_last_four) }}</code>
              <span>·</span>
              <span>{{ token.scopes.length }} スコープ</span>
              <span>·</span>
              <span>{{ formatExpiry(token.expires_at) }}</span>
            </p>
            <p class="text-muted-foreground text-xs">{{ formatLastUsed(token.last_used_at) }}</p>
          </div>
          <DropdownMenu>
            <DropdownMenuTrigger as-child>
              <Button type="button" variant="outline" size="sm">
                スコープを表示
                <PhCaretDown class="size-3" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem v-for="scope in token.scopes" :key="scope" disabled>
                <code class="font-mono text-xs">{{ scope }}</code>
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
          <Button
            type="button"
            variant="outline"
            size="sm"
            class="text-destructive hover:text-destructive"
            @click="openRevokeDialog(token)"
          >
            取り消し
          </Button>
        </li>
      </ul>
    </template>

    <!-- 取り消し確認 -->
    <Dialog v-if="revokeTarget" :open="true" @update:open="onRevokeOpenChange">
      <DialogContent class="max-w-md" :show-close-button="false">
        <DialogHeader>
          <DialogTitle>トークンを取り消しますか？</DialogTitle>
          <DialogDescription>
            「{{ revokeTarget.name }}」を取り消します。このトークンを使っている API・CLI
            は即座に認証できなくなります。この操作は元に戻せません。
          </DialogDescription>
        </DialogHeader>
        <p v-if="revokeError" class="text-destructive text-sm">{{ revokeError }}</p>
        <DialogFooter>
          <Button
            type="button"
            variant="outline"
            :disabled="revokeToken.isPending.value"
            @click="revokeTarget = null"
          >
            キャンセル
          </Button>
          <Button
            type="button"
            variant="destructive"
            :disabled="revokeToken.isPending.value"
            @click="onRevokeConfirm"
          >
            {{ revokeToken.isPending.value ? '取り消し中…' : '取り消す' }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>
