<script setup lang="ts">
import { useQueryClient } from '@tanstack/vue-query';
import { PhEnvelopeSimple, PhPaperPlaneTilt, PhX } from '@phosphor-icons/vue';
import { computed, ref } from 'vue';

import { Button } from '@/components/ui/button';
import { Select, SelectContent, SelectItem, SelectTrigger } from '@/components/ui/select';
import { apiClient, useMeQuery } from '@/lib/api-vue-query';
import type { components } from '@/generated/api';
import { useAuthStore } from '@/stores/auth';

type TenantResponse = components['schemas']['TenantResponse'];
type TenantMemberResponse = components['schemas']['TenantMemberResponse'];
type TenantRole = components['schemas']['TenantRole'];

const MEMBERS_PATH = '/v1/tenants/{tenant_id}/members' as const;
const MEMBER_PATH = '/v1/tenants/{tenant_id}/members/{user_id}' as const;

/**
 * ロールを見ているのは今のところテナントメンバー管理 API だけで、Viewer と Member に
 * 書き込みの差は無い（`apps/backend/docs/tenant-project-authz.md`）。読み取り専用が
 * 全経路で効くまでは、権限が絞られたと誤解させない書き方にしておく。
 */
const ROLES: { value: TenantRole; label: string; description: string }[] = [
  { value: 'Admin', label: 'Admin', description: 'メンバーの追加・変更・削除ができる' },
  { value: 'Member', label: 'Member', description: 'タスクの作成と編集ができる' },
  {
    value: 'Viewer',
    label: 'Viewer',
    description: '今は Member と同じ（読み取り専用は未実装）',
  },
];

const props = defineProps<{ tenant: TenantResponse }>();

const queryClient = useQueryClient();
const authStore = useAuthStore();

const membersQuery = apiClient.useQuery(
  'get',
  MEMBERS_PATH,
  {
    params: { path: { tenant_id: props.tenant.id } },
  },
  {
    staleTime: 60_000,
    retry: false,
  },
);
const meQuery = useMeQuery();
const currentUser = computed(() => meQuery.data.value ?? authStore.user);

const addMutation = apiClient.useMutation('post', MEMBERS_PATH);
const updateMutation = apiClient.useMutation('put', MEMBER_PATH);
const removeMutation = apiClient.useMutation('delete', MEMBER_PATH);

/**
 * オーナーは認可上 tenant_members に行を持たない。現行 API は一覧用の synthetic row
 * を返すが、旧レスポンスや更新途中でも owner が欠けないよう本人情報から補完する。
 */
const members = computed<TenantMemberResponse[]>(() => {
  const currentMembers = [...(membersQuery.data.value ?? [])];
  const me = currentUser.value;
  if (me?.id === props.tenant.owner_id && !currentMembers.some((m) => m.user_id === me.id)) {
    currentMembers.unshift({
      id: me.id,
      tenant_id: props.tenant.id,
      user_id: me.id,
      role: 'Admin',
      user: { id: me.id, username: me.username, avatar_url: me.avatar_url ?? null },
    });
  }
  return currentMembers;
});
const memberCount = computed(() => members.value.length);

const canManageMembers = computed(() => {
  const userId = currentUser.value?.id;
  if (!userId) return false;
  if (userId === props.tenant.owner_id) return true;
  return members.value.some((member) => member.user_id === userId && member.role === 'Admin');
});

async function invalidateMembers() {
  await queryClient.invalidateQueries({ queryKey: ['get', MEMBERS_PATH] });
}

/** オーナーは外せず、ロールも変えられない。行の見た目もそこで分ける。 */
function isOwner(member: TenantMemberResponse) {
  return member.user_id === props.tenant.owner_id;
}

function isSelf(member: TenantMemberResponse) {
  return member.user_id === currentUser.value?.id;
}

function roleDescription(role: TenantRole) {
  return ROLES.find((entry) => entry.value === role)?.description ?? '';
}

/** アバターの頭文字。表示名が無いときはユーザー名の先頭 2 文字を使う。 */
function initials(member: TenantMemberResponse) {
  return member.user.username.slice(0, 2).toUpperCase();
}

/** 頭文字から色を決める。同じ人はいつも同じ色になる。 */
const AVATAR_COLORS = ['#0f766e', '#7c3aed', '#b45309', '#be123c', '#1d4ed8', '#4d7c0f'];
function avatarColor(member: TenantMemberResponse) {
  const sum = [...member.user.username].reduce((acc, char) => acc + char.charCodeAt(0), 0);
  return AVATAR_COLORS[sum % AVATAR_COLORS.length];
}

// --- 招待 ---
//
// 招待の API はまだ無い（`add_member` は user_id を受け取るので、メールでは呼べない）。
// API ができるまで入力欄と送信ボタンは無効にしておく。

const inviteEmail = ref('');
const inviteRole = ref<TenantRole>('Member');

// --- ロール変更 ---

const roleError = ref<string | null>(null);

async function onRoleChange(member: TenantMemberResponse, role: TenantRole) {
  if (!canManageMembers.value || isOwner(member)) return;
  if (role === member.role) return;
  roleError.value = null;
  try {
    await updateMutation.mutateAsync({
      params: { path: { tenant_id: props.tenant.id, user_id: member.user_id } },
      body: { role },
    });
    await invalidateMembers();
  } catch {
    roleError.value = 'ロールを変更できませんでした。';
    // 表示は membersQuery のデータに束縛しているので、再取得で元のロールへ戻る
    await invalidateMembers();
  }
}

// --- 削除 ---

const removeError = ref<string | null>(null);

async function onRemove(member: TenantMemberResponse) {
  if (!canManageMembers.value || isOwner(member)) return;
  removeError.value = null;
  try {
    await removeMutation.mutateAsync({
      params: { path: { tenant_id: props.tenant.id, user_id: member.user_id } },
    });
    await invalidateMembers();
  } catch {
    removeError.value = 'メンバーを外せませんでした。';
  }
}
</script>

<template>
  <div class="min-h-0 flex-1 overflow-auto">
    <div class="mx-auto max-w-[760px] px-6 pb-14 pt-8">
      <div class="mb-6">
        <h1 class="m-0 mb-1 text-2xl font-bold tracking-tight">メンバー</h1>
        <p class="m-0 text-sm text-muted-foreground">
          <strong class="font-medium text-foreground">{{ tenant.name }}</strong>
          に入れる人を管理します。メンバーはこのテナントのすべてのプロジェクトを見られます。
        </p>
      </div>

      <!-- 招待 -->
      <section class="mb-7 rounded-[10px] border p-4">
        <h2 class="mb-3 text-sm font-semibold">人を招待する</h2>
        <div class="flex flex-wrap items-stretch gap-2">
          <div class="relative min-w-[220px] flex-1">
            <PhEnvelopeSimple
              class="pointer-events-none absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground"
            />
            <input
              v-model="inviteEmail"
              type="email"
              disabled
              aria-label="招待するメールアドレス"
              placeholder="name@example.com"
              class="h-9 w-full rounded-md border bg-background pl-8 pr-3 text-sm shadow-sm outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50"
            />
          </div>
          <Select v-model="inviteRole" disabled>
            <SelectTrigger aria-label="招待するロール" class="h-9 w-[130px]">
              <span class="truncate">{{ inviteRole }}</span>
            </SelectTrigger>
            <SelectContent>
              <SelectItem v-for="role in ROLES" :key="role.value" :value="role.value">
                {{ role.label }}
              </SelectItem>
            </SelectContent>
          </Select>
          <Button class="gap-2" disabled>
            <PhPaperPlaneTilt class="size-4" />
            招待
          </Button>
        </div>
        <p class="mt-2.5 text-xs text-muted-foreground">
          招待機能は準備中です。現在はメールを送信できません。
        </p>
      </section>

      <!-- メンバー -->
      <section>
        <div class="mb-2.5 flex items-baseline gap-2">
          <h2 class="text-sm font-semibold">メンバー</h2>
          <span class="text-xs text-muted-foreground">{{ memberCount }}</span>
        </div>

        <p v-if="membersQuery.isPending.value" class="text-sm text-muted-foreground">
          メンバーを読み込み中…
        </p>
        <p v-else-if="membersQuery.isError.value" role="alert" class="text-sm text-destructive">
          メンバーを読み込めませんでした
        </p>

        <template v-else>
          <p v-if="!canManageMembers" class="mb-2 text-sm text-muted-foreground" role="status">
            メンバーのロール変更と除外ができるのは、テナントオーナーと Admin だけです。
          </p>

          <p v-if="roleError" role="alert" class="mb-2 text-sm text-destructive">{{ roleError }}</p>
          <p v-if="removeError" role="alert" class="mb-2 text-sm text-destructive">
            {{ removeError }}
          </p>

          <ul class="rounded-[10px] border">
            <li
              v-for="member in members"
              :key="member.id"
              class="flex items-center gap-3 border-b px-3.5 py-2.5 last:border-b-0"
            >
              <span
                class="flex size-[34px] shrink-0 items-center justify-center rounded-lg text-xs font-semibold text-white"
                :style="{ background: avatarColor(member) }"
                aria-hidden="true"
              >
                {{ initials(member) }}
              </span>
              <span class="min-w-0 flex-1 overflow-hidden leading-snug">
                <span class="block truncate text-sm font-medium">
                  {{ member.user.username }}
                  <span v-if="isSelf(member)" class="text-xs font-normal text-muted-foreground">
                    (あなた)
                  </span>
                </span>
                <!--
                  参照デザインはここにメールを出すが、`UserSummary` は
                  id / username / avatar_url しか返さない。UUID を出しても読めないので、
                  API がメールを持つまでは説明にロールの意味を出す。
                -->
                <span class="block truncate text-xs text-muted-foreground">
                  {{ roleDescription(member.role) }}
                </span>
              </span>

              <span
                v-if="isOwner(member)"
                class="shrink-0 px-2.5 text-[13px] text-muted-foreground"
              >
                オーナー
              </span>

              <template v-else>
                <Select
                  :model-value="member.role"
                  :disabled="!canManageMembers"
                  @update:model-value="onRoleChange(member, $event as TenantRole)"
                >
                  <SelectTrigger
                    :aria-label="`${member.user.username}のロール`"
                    class="h-8 w-[110px] shrink-0"
                  >
                    <!--
                      `SelectValue` は選んだ項目の中身をそのまま写すため、説明まで
                      引き金に出てしまう（「Adminメン…」のような潰れた表示になる）。
                      引き金にはロール名だけを置く。
                    -->
                    <span class="truncate text-[13px]">{{ member.role }}</span>
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem v-for="role in ROLES" :key="role.value" :value="role.value">
                      <span class="block text-sm font-medium">{{ role.label }}</span>
                      <span class="block text-xs text-muted-foreground">
                        {{ role.description }}
                      </span>
                    </SelectItem>
                  </SelectContent>
                </Select>
                <Button
                  variant="ghost"
                  size="icon"
                  :disabled="!canManageMembers"
                  class="size-7 shrink-0 text-muted-foreground"
                  :aria-label="`${member.user.username}を外す`"
                  @click="onRemove(member)"
                >
                  <PhX class="size-3.5" />
                </Button>
              </template>
            </li>
          </ul>

          <p v-if="members.length === 0" class="py-6 text-center text-sm text-muted-foreground">
            メンバーがいません
          </p>
        </template>
      </section>
    </div>
  </div>
</template>
