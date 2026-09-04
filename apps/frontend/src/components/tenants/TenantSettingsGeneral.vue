<script setup lang="ts">
import { useQueryClient } from '@tanstack/vue-query';
import { computed, reactive, ref, watch } from 'vue';

import TenantIconPicker from '@/components/tenants/TenantIconPicker.vue';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Textarea } from '@/components/ui/textarea';
import DeleteTenantDialog from '@/components/tenants/DeleteTenantDialog.vue';
import { apiClient } from '@/lib/api-vue-query';
import type { components } from '@/generated/api';

type TenantResponse = components['schemas']['TenantResponse'];

const LIST_TENANTS_PATH = '/v1/tenants' as const;
const TENANT_PATH = '/v1/tenants/{id}' as const;

const props = defineProps<{ tenant: TenantResponse }>();

const queryClient = useQueryClient();
const submitError = ref<string | null>(null);
const isDeleteOpen = ref(false);

/**
 * 保存はページ下の固定バーからまとめて行う。
 * どれか 1 つでも変わったら出す必要があるので、編集中の値は 1 つの箱に集める。
 */
const draft = reactive({
  name: props.tenant.name,
  description: props.tenant.description,
  iconUrl: props.tenant.icon_url,
  emoji: '🗂️',
});

/** テナントを切り替えたら編集中の値も差し替える（前のテナントの入力を持ち越さない）。 */
watch(
  () => props.tenant,
  (tenant) => {
    draft.name = tenant.name;
    draft.description = tenant.description;
    draft.iconUrl = tenant.icon_url;
  },
);

const isDirty = computed(
  () =>
    draft.name !== props.tenant.name ||
    draft.description !== props.tenant.description ||
    draft.iconUrl !== props.tenant.icon_url,
);

const canSave = computed(() => isDirty.value && draft.name.trim().length > 0);

const updateMutation = apiClient.useMutation('put', TENANT_PATH);

function discard() {
  draft.name = props.tenant.name;
  draft.description = props.tenant.description;
  draft.iconUrl = props.tenant.icon_url;
  submitError.value = null;
}

async function save() {
  if (!canSave.value) return;
  submitError.value = null;
  try {
    await updateMutation.mutateAsync({
      params: { path: { id: props.tenant.id } },
      body: {
        name: draft.name.trim(),
        description: draft.description,
        icon_url: draft.iconUrl,
      },
    });
    await queryClient.invalidateQueries({ queryKey: ['get', LIST_TENANTS_PATH] });
  } catch {
    submitError.value = 'テナントを更新できませんでした';
  }
}

/**
 * 既定値とアクセス。API に持ち場がまだ無いので、画面の形だけを先に作る。
 * 保存対象にも入れていない（`isDirty` が見ているのは API に送る 3 項目だけ）。
 */
const defaults = reactive({
  timezone: 'Asia/Tokyo (JST)',
  weekStart: '月曜',
  priority: 'Medium',
  visibility: 'テナントのメンバー',
});

const DEFAULT_FIELDS = [
  {
    key: 'timezone',
    label: 'タイムゾーン',
    choices: ['Asia/Tokyo (JST)', 'UTC', 'America/Los_Angeles (PT)', 'Europe/Berlin (CET)'],
  },
  { key: 'weekStart', label: '週の開始', choices: ['月曜', '日曜', '土曜'] },
  { key: 'priority', label: 'タスクの既定の優先度', choices: ['High', 'Medium', 'Low'] },
  {
    key: 'visibility',
    label: 'プロジェクトの既定の公開範囲',
    choices: ['テナントのメンバー', 'プロジェクトのメンバーのみ'],
  },
] as const;

const flags = reactive({
  openJoin: true,
  guestLinks: false,
  requireVerify: true,
});

const ACCESS_FLAGS = [
  {
    key: 'openJoin',
    label: 'メールのドメインで参加を許す',
    desc: '同じドメインのアドレスを持つ人が、招待なしで参加できます。',
  },
  {
    key: 'guestLinks',
    label: 'ゲスト用の共有リンクを許す',
    desc: 'メンバーが読み取り専用のリンクをテナントの外へ共有できます。',
  },
  {
    key: 'requireVerify',
    label: 'メールの確認を必須にする',
    desc: 'アドレスを確認するまでタスクを開けません。',
  },
] as const;
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col">
    <div class="min-h-0 flex-1 overflow-auto">
      <div class="mx-auto max-w-[760px] px-6 pb-14 pt-8">
        <div class="mb-6">
          <h1 class="m-0 mb-1 text-2xl font-bold tracking-tight">一般</h1>
          <p class="m-0 text-sm text-muted-foreground">
            テナント全体の情報と既定値です。すべてのプロジェクトとメンバーに適用されます。
          </p>
        </div>

        <!-- Identity -->
        <section class="mb-5 rounded-[10px] border p-4">
          <h2 class="mb-3.5 text-sm font-semibold">基本情報</h2>

          <div class="mb-4 flex items-start gap-4">
            <div class="shrink-0">
              <div class="mb-1.5 text-[13px] font-medium">アイコン</div>
              <TenantIconPicker v-model:emoji="draft.emoji" v-model:image-url="draft.iconUrl" />
            </div>
            <div class="min-w-0 flex-1">
              <Label for="tenant-name" class="mb-1.5 text-[13px] font-medium">テナント名</Label>
              <Input id="tenant-name" v-model="draft.name" placeholder="Acme Inc" class="h-9" />
            </div>
          </div>

          <div class="mb-4">
            <Label for="tenant-slug" class="mb-1.5 text-[13px] font-medium">URL</Label>
            <div class="flex h-9 items-center overflow-hidden rounded-md border shadow-sm">
              <span
                class="flex h-full shrink-0 items-center border-r bg-secondary px-2.5 font-mono text-[13px] text-muted-foreground"
              >
                task.app/
              </span>
              <input
                id="tenant-slug"
                :value="props.tenant.display_id"
                disabled
                class="h-full min-w-0 flex-1 bg-transparent px-2.5 font-mono text-[13px] text-muted-foreground outline-none"
              />
            </div>
            <p class="mt-1.5 text-xs text-muted-foreground">
              URL は作成時に決まり、あとから変えられません。
            </p>
          </div>

          <div>
            <Label for="tenant-description" class="mb-1.5 text-[13px] font-medium">説明</Label>
            <Textarea
              id="tenant-description"
              v-model="draft.description"
              rows="2"
              placeholder="このテナントの用途"
            />
          </div>
        </section>

        <!-- Defaults -->
        <section class="mb-5 rounded-[10px] border p-4">
          <h2 class="mb-1 text-sm font-semibold">既定値</h2>
          <p class="mb-3.5 text-xs text-muted-foreground">
            このテナントで新しく作るプロジェクトとタスクに使われます。
          </p>
          <div class="grid grid-cols-2 gap-3">
            <div v-for="field in DEFAULT_FIELDS" :key="field.key" class="min-w-0">
              <Label :for="`tenant-default-${field.key}`" class="mb-1.5 text-[13px] font-medium">
                {{ field.label }}
              </Label>
              <Select v-model="defaults[field.key]">
                <SelectTrigger :id="`tenant-default-${field.key}`" class="h-9 w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem v-for="choice in field.choices" :key="choice" :value="choice">
                    {{ choice }}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>
        </section>

        <!-- Access -->
        <section class="mb-5 rounded-[10px] border">
          <h2 class="px-4 pt-4 text-sm font-semibold">アクセス</h2>
          <div class="px-4 pb-1.5 pt-2.5">
            <div
              v-for="flag in ACCESS_FLAGS"
              :key="flag.key"
              class="flex items-start gap-3 border-b py-2.5 last:border-b-0"
            >
              <span class="min-w-0 flex-1 leading-snug">
                <span class="block text-sm font-medium">{{ flag.label }}</span>
                <span class="block text-xs text-muted-foreground">{{ flag.desc }}</span>
              </span>
              <button
                type="button"
                role="switch"
                :aria-checked="flags[flag.key]"
                :aria-label="flag.label"
                class="relative mt-0.5 h-5 w-9 shrink-0 rounded-full border transition-colors"
                :class="flags[flag.key] ? 'border-primary bg-primary' : 'border-input bg-secondary'"
                @click="flags[flag.key] = !flags[flag.key]"
              >
                <span
                  class="absolute top-px size-4 rounded-full bg-white transition-[left]"
                  :class="flags[flag.key] ? 'left-[17px]' : 'left-px'"
                />
              </button>
            </div>
          </div>
        </section>

        <!-- Danger zone -->
        <section class="rounded-[10px] border p-4">
          <h2 class="mb-3.5 text-sm font-semibold">危険な操作</h2>

          <div class="flex items-center gap-3 border-b pb-3.5">
            <span class="min-w-0 flex-1 leading-snug">
              <span class="block text-sm font-medium">オーナーを移す</span>
              <span class="block text-xs text-muted-foreground">
                このテナントを別の管理者に渡します。自分はメンバーとして残ります。
              </span>
            </span>
            <Button variant="outline" size="sm" class="shrink-0">移す</Button>
          </div>

          <div class="flex items-center gap-3 pt-3.5">
            <span class="min-w-0 flex-1 leading-snug">
              <span class="block text-sm font-medium">テナントを削除</span>
              <span class="block text-xs text-muted-foreground">
                プロジェクト・タスク・メンバーの権限をすべて消します。
              </span>
            </span>
            <Button variant="destructive" size="sm" class="shrink-0" @click="isDeleteOpen = true">
              削除
            </Button>
          </div>
        </section>
      </div>
    </div>

    <!-- 保存バー。変更があるときだけ出す -->
    <div v-if="isDirty" class="flex shrink-0 items-center gap-3 border-t bg-background px-6 py-3">
      <span
        class="flex-1 text-[13px]"
        :class="submitError ? 'text-destructive' : 'text-muted-foreground'"
      >
        {{ submitError ?? '保存されていない変更があります' }}
      </span>
      <Button variant="outline" @click="discard">取り消す</Button>
      <Button :disabled="!canSave || updateMutation.isPending.value" @click="save">
        変更を保存
      </Button>
    </div>

    <DeleteTenantDialog v-model:open="isDeleteOpen" :tenant="props.tenant" />
  </div>
</template>
