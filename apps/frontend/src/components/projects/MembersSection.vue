<script setup lang="ts">
import { useQueryClient } from '@tanstack/vue-query';
import { PhPlus } from '@phosphor-icons/vue';
import { computed, ref } from 'vue';
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { LIST_PROJECTS_PATH, apiClient, useMeQuery } from '@/lib/api-vue-query';
import type { components } from '@/generated/api';

type ProjectMemberResponse = components['schemas']['ProjectMemberResponse'];
type ProjectRole = components['schemas']['ProjectRole'];

const MEMBERS_PATH = '/v1/tenants/{tenant_id}/projects/{project_id}/members' as const;
const MEMBER_PATH = '/v1/tenants/{tenant_id}/projects/{project_id}/members/{user_id}' as const;
const TENANT_MEMBERS_PATH = '/v1/tenants/{tenant_id}/members' as const;

const ROLES: { value: ProjectRole; label: string; description: string }[] = [
  { value: 'Admin', label: '管理者', description: 'メンバーの管理を含むすべての操作' },
  { value: 'Member', label: 'メンバー', description: 'プロジェクトに参加して作業する' },
  { value: 'Viewer', label: '閲覧者', description: 'プロジェクトを閲覧する' },
];

function roleLabel(role: ProjectRole) {
  return ROLES.find((r) => r.value === role)?.label ?? role;
}

const props = defineProps<{
  tenantId: string;
  projectId: string;
}>();

const queryClient = useQueryClient();

const pathParams = {
  params: { path: { tenant_id: props.tenantId, project_id: props.projectId } },
};
const membersQuery = apiClient.useQuery('get', MEMBERS_PATH, pathParams, { retry: false });
const tenantMembersQuery = apiClient.useQuery(
  'get',
  TENANT_MEMBERS_PATH,
  { params: { path: { tenant_id: props.tenantId } } },
  { retry: false },
);

const members = computed(() => membersQuery.data.value ?? []);

/** 一覧・追加・変更・削除はプロジェクト管理者（またはオーナー）専用。 */
const isForbidden = computed(
  () =>
    (membersQuery.error.value as { response?: { status?: number } } | null)?.response?.status ===
    403,
);

/**
 * 追加候補 = テナントメンバーのうち、まだプロジェクトメンバーでない人。
 *
 * 取得に失敗したときも空になるので、「候補がいません」と出す前に
 * `tenantMembersQuery.isError` を見る（失敗を「テナントに人が居ない」という
 * 別の事実にすり替えない）。
 */
const candidates = computed(() => {
  const existing = new Set(members.value.map((m) => m.user_id));
  return (tenantMembersQuery.data.value ?? []).filter((tm) => !existing.has(tm.user_id));
});

const addMutation = apiClient.useMutation('post', MEMBERS_PATH);
const updateMutation = apiClient.useMutation('put', MEMBER_PATH);
const removeMutation = apiClient.useMutation('delete', MEMBER_PATH);

const meQuery = useMeQuery();

/** 対象が自分か。自分を外すとこの画面を開けなくなるので、削除の前に伝える。 */
function isSelf(member: ProjectMemberResponse) {
  return member.user_id === meQuery.data.value?.id;
}

/**
 * メンバーの増減はプロジェクトの見え方も変える。
 *
 * - 自分を外せば、そのプロジェクトは一覧から消える
 * - 最後の 1 人を外せば `project_members` が 0 件になり、テナントメンバー全員に開く
 *
 * どちらもプロジェクト一覧の中身が変わるので、メンバーだけでなく一覧も無効化する
 * （しないと、サイドバーや設定の他セクションが古い権限のまま操作できる顔で残る）。
 */
async function invalidateMembers() {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: ['get', MEMBERS_PATH] }),
    queryClient.invalidateQueries({ queryKey: ['get', LIST_PROJECTS_PATH] }),
  ]);
}

function errorStatus(e: unknown): number | undefined {
  return (e as { response?: { status?: number } }).response?.status;
}

// --- 追加 ---

const selectedUserId = ref<string | null>(null);
const selectedRole = ref<ProjectRole>('Member');
const addError = ref<string | null>(null);

async function onAdd() {
  const userId = selectedUserId.value;
  if (!userId) return;
  addError.value = null;
  try {
    await addMutation.mutateAsync({
      params: { path: { tenant_id: props.tenantId, project_id: props.projectId } },
      body: { user_id: userId, role: selectedRole.value },
    });
    await invalidateMembers();
    selectedUserId.value = null;
    selectedRole.value = 'Member';
  } catch (e) {
    const status = errorStatus(e);
    addError.value =
      status === 409
        ? 'この利用者は既にメンバーです。'
        : status === 400
          ? 'テナントメンバーでない利用者は追加できません。'
          : 'メンバーを追加できませんでした。';
  }
}

// --- ロール変更 ---

const roleError = ref<string | null>(null);

async function onRoleChange(member: ProjectMemberResponse, role: ProjectRole) {
  if (role === member.role) return;
  roleError.value = null;
  try {
    await updateMutation.mutateAsync({
      params: {
        path: {
          tenant_id: props.tenantId,
          project_id: props.projectId,
          user_id: member.user_id,
        },
      },
      body: { role },
    });
    await invalidateMembers();
  } catch (e) {
    roleError.value =
      errorStatus(e) === 409
        ? `最後の管理者「${member.user.username}」は降格できません。`
        : 'ロールを変更できませんでした。';
    // 表示は membersQuery のデータに束縛しているため、失敗時は再取得で元のロールに戻る
    await invalidateMembers();
  }
}

// --- 削除 ---

const removeTarget = ref<ProjectMemberResponse | null>(null);
const removeError = ref<string | null>(null);

function openRemove(member: ProjectMemberResponse) {
  removeError.value = null;
  removeTarget.value = member;
}

function onRemoveOpenChange(open: boolean) {
  // 削除リクエスト進行中はダイアログを閉じない（結果の見逃し防止）
  if (!open && removeMutation.isPending.value) return;
  if (!open) removeTarget.value = null;
}

async function confirmRemove() {
  const target = removeTarget.value;
  if (!target) return;
  removeError.value = null;
  try {
    await removeMutation.mutateAsync({
      params: {
        path: {
          tenant_id: props.tenantId,
          project_id: props.projectId,
          user_id: target.user_id,
        },
      },
    });
    await invalidateMembers();
    removeTarget.value = null;
  } catch (e) {
    removeError.value =
      errorStatus(e) === 409
        ? '最後の管理者は削除できません。'
        : 'メンバーを削除できませんでした。';
  }
}

function initials(username: string) {
  return username.slice(0, 1).toUpperCase();
}
</script>

<template>
  <div>
    <div class="mb-5 border-b pb-4">
      <h2 class="text-xl font-semibold">メンバー</h2>
      <p class="mt-1 text-sm text-muted-foreground">
        メンバーを 1 人も指定しない間は、テナントメンバー全員がこのプロジェクトに入れます。
        指定すると、ここに載っている人だけに絞られます。
      </p>
    </div>

    <p v-if="membersQuery.isPending.value" class="text-sm text-muted-foreground">読み込み中…</p>

    <p v-else-if="isForbidden" class="text-sm text-muted-foreground">
      メンバーを管理できるのは、テナントオーナーとこのプロジェクトの管理者だけです。
    </p>

    <p v-else-if="membersQuery.isError.value" role="alert" class="text-sm text-destructive">
      メンバーを読み込めませんでした
    </p>

    <template v-else>
      <!-- 追加フォーム -->
      <div class="mb-5 flex flex-wrap items-end gap-2">
        <div class="min-w-[220px] flex-1">
          <label for="member-candidate" class="mb-1.5 block text-sm font-medium">
            テナントメンバーから追加
          </label>
          <Select
            :model-value="selectedUserId ?? undefined"
            @update:model-value="(v) => (selectedUserId = v == null ? null : String(v))"
          >
            <SelectTrigger id="member-candidate" class="w-full">
              <SelectValue placeholder="利用者を選択" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem
                v-for="candidate in candidates"
                :key="candidate.user_id"
                :value="candidate.user_id"
              >
                {{ candidate.user.username }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>
        <div class="w-[140px]">
          <label for="member-role" class="mb-1.5 block text-sm font-medium">ロール</label>
          <Select
            :model-value="selectedRole"
            @update:model-value="(v) => (selectedRole = v as ProjectRole)"
          >
            <SelectTrigger id="member-role" class="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem v-for="role in ROLES" :key="role.value" :value="role.value">
                {{ role.label }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>
        <Button
          type="button"
          :disabled="!selectedUserId || addMutation.isPending.value"
          @click="onAdd"
        >
          <PhPlus class="size-4" />
          {{ addMutation.isPending.value ? '追加中…' : '追加' }}
        </Button>
      </div>
      <p
        v-if="tenantMembersQuery.isError.value"
        role="alert"
        class="-mt-3 mb-5 text-sm text-destructive"
      >
        追加候補を読み込めませんでした
      </p>
      <p
        v-else-if="!candidates.length && !tenantMembersQuery.isPending.value"
        class="-mt-3 mb-5 text-xs text-muted-foreground"
      >
        追加できるテナントメンバーがいません。先にテナントへメンバーを追加してください。
      </p>
      <p v-if="addError" role="alert" class="-mt-3 mb-5 text-sm text-destructive">
        {{ addError }}
      </p>

      <p v-if="roleError" role="alert" class="mb-3 text-sm text-destructive">{{ roleError }}</p>

      <p v-if="members.length === 0" class="text-sm text-muted-foreground">
        メンバーはまだ指定されていません。
      </p>

      <ul v-else class="overflow-hidden rounded-lg border" data-testid="member-list">
        <li
          v-for="member in members"
          :key="member.id"
          class="flex items-center gap-3 border-b px-3.5 py-2.5 last:border-b-0"
        >
          <Avatar class="size-8">
            <AvatarImage
              v-if="member.user.avatar_url"
              :src="member.user.avatar_url"
              :alt="member.user.username"
            />
            <AvatarFallback class="bg-muted text-xs text-muted-foreground">
              {{ initials(member.user.username) }}
            </AvatarFallback>
          </Avatar>
          <span class="min-w-0 flex-1 truncate text-sm font-medium">
            {{ member.user.username }}
          </span>
          <Select
            :model-value="member.role"
            :disabled="updateMutation.isPending.value"
            @update:model-value="(v) => onRoleChange(member, v as ProjectRole)"
          >
            <SelectTrigger
              class="w-[130px]"
              :aria-label="`${member.user.username} のロール`"
              size="sm"
            >
              <SelectValue>{{ roleLabel(member.role) }}</SelectValue>
            </SelectTrigger>
            <SelectContent>
              <SelectItem v-for="role in ROLES" :key="role.value" :value="role.value">
                <span class="flex flex-col">
                  <span>{{ role.label }}</span>
                  <span class="text-xs text-muted-foreground">{{ role.description }}</span>
                </span>
              </SelectItem>
            </SelectContent>
          </Select>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            class="text-destructive hover:text-destructive"
            :aria-label="`メンバー「${member.user.username}」を削除`"
            @click="openRemove(member)"
          >
            削除
          </Button>
        </li>
      </ul>
    </template>

    <Dialog v-if="removeTarget" :open="true" @update:open="onRemoveOpenChange">
      <DialogContent class="max-w-md" :show-close-button="false">
        <DialogHeader>
          <DialogTitle>メンバーを削除しますか？</DialogTitle>
          <DialogDescription>
            「{{ removeTarget.user.username }}」をこのプロジェクトから外します。
            担当タスクの割り当ては残りますが、ウォッチしていたタスクの通知は解除されます。
          </DialogDescription>
          <DialogDescription v-if="members.length === 1" class="text-destructive">
            これが最後のメンバーです。外すとメンバー指定が無くなり、このプロジェクトは
            テナントメンバー全員が開けるようになります。
          </DialogDescription>
          <DialogDescription v-if="isSelf(removeTarget)" class="text-destructive">
            自分をこのプロジェクトから外します。以後この設定画面は開けなくなります。
          </DialogDescription>
        </DialogHeader>
        <p v-if="removeError" role="alert" class="text-sm text-destructive">{{ removeError }}</p>
        <DialogFooter>
          <Button
            type="button"
            variant="outline"
            :disabled="removeMutation.isPending.value"
            @click="removeTarget = null"
          >
            キャンセル
          </Button>
          <Button
            type="button"
            variant="destructive"
            :disabled="removeMutation.isPending.value"
            @click="confirmRemove"
          >
            {{ removeMutation.isPending.value ? '削除中…' : '削除する' }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>
