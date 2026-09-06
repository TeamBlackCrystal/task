<script setup lang="ts">
import { useForm } from '@tanstack/vue-form';
import { type } from 'arktype';
import { useQueryClient } from '@tanstack/vue-query';
import {
  PhGear,
  PhKanban,
  PhPlugsConnected,
  PhTag,
  PhTextbox,
  PhUsers,
  PhWarning,
} from '@phosphor-icons/vue';
import { navigate } from 'vike/client/router';
import { usePageContext } from 'vike-vue/usePageContext';
import { computed, ref, watch, type Component } from 'vue';
import { Button } from '@/components/ui/button';
import { Field, FieldError, FieldLabel } from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import DeleteProjectDialog from '@/components/sidebar/DeleteProjectDialog.vue';
import CustomFieldsSection from '@/components/projects/CustomFieldsSection.vue';
import EmojiIconPicker from '@/components/projects/EmojiIconPicker.vue';
import IntegrationsSection from '@/components/projects/IntegrationsSection.vue';
import LabelsSection from '@/components/projects/LabelsSection.vue';
import MembersSection from '@/components/projects/MembersSection.vue';
import WorkflowStatusesEditor from '@/components/projects/WorkflowStatusesEditor.vue';
import { apiClient } from '@/lib/api-vue-query';
import type { components } from '@/generated/api';

type ProjectResponse = components['schemas']['ProjectResponse'];

const LIST_PROJECTS_PATH = '/v1/tenants/{tenant_id}/projects' as const;
const PROJECT_PATH = '/v1/tenants/{tenant_id}/projects/{id}' as const;

/** 設定セクション。残りは増分で追加 */
type SettingsSection =
  | 'general'
  | 'members'
  | 'workflow'
  | 'labels'
  | 'fields'
  | 'integrations'
  | 'danger';

const props = defineProps<{
  tenantId: string;
  tenantSlug: string;
  project: ProjectResponse;
}>();

const queryClient = useQueryClient();
const pageContext = usePageContext();
const submitError = ref<string | null>(null);
const saveDone = ref(false);
const isDeleteOpen = ref(false);
const icon = ref<string | null>(props.project.icon_emoji ?? null);

/**
 * アイコンは section ごとに持たせる。テンプレート側で key を条件分岐していたときは
 * 当てはまらない節が全部フォールバックの 1 つに落ち、一般・ラベル・カスタムフィールド・
 * 連携の 4 つが同じ絵になっていた。ここで必須にすれば足し忘れが型で分かる。
 */
const sections: {
  key: SettingsSection;
  label: string;
  icon: Component;
  danger?: boolean;
}[] = [
  { key: 'general', label: '一般', icon: PhGear },
  { key: 'members', label: 'メンバー', icon: PhUsers },
  { key: 'workflow', label: 'ワークフロー', icon: PhKanban },
  { key: 'labels', label: 'ラベル', icon: PhTag },
  { key: 'fields', label: 'カスタムフィールド', icon: PhTextbox },
  { key: 'integrations', label: '連携', icon: PhPlugsConnected },
  { key: 'danger', label: '削除', icon: PhWarning, danger: true },
];

/** `?section=` から初期表示セクションを決める（GitHub callback の戻り先が利用。#386） */
function initialSection(): SettingsSection {
  const search = (pageContext as { urlParsed?: { search?: Record<string, string> } } | undefined)
    ?.urlParsed?.search;
  const requested = search?.section;
  return sections.some((section) => section.key === requested)
    ? (requested as SettingsSection)
    : 'general';
}

const activeSection = ref<SettingsSection>(initialSection());

const nonBlankName = type('string').narrow((name) => name.trim().length >= 1);

const schema = type({
  name: nonBlankName,
  description: 'string',
});

const updateMutation = apiClient.useMutation('put', PROJECT_PATH);

const form = useForm({
  defaultValues: {
    name: props.project.name,
    description: props.project.description,
  },
  validators: { onSubmit: schema },
  onSubmit: async ({ value }) => {
    submitError.value = null;
    saveDone.value = false;
    try {
      const originalIcon = props.project.icon_emoji ?? null;
      await updateMutation.mutateAsync({
        params: { path: { tenant_id: props.tenantId, id: props.project.id } },
        body: {
          name: value.name.trim(),
          description: value.description.trim(),
          ...(icon.value && icon.value !== originalIcon ? { icon_emoji: icon.value } : {}),
          ...(!icon.value && originalIcon ? { clear_icon_emoji: true } : {}),
        },
      });
      await queryClient.invalidateQueries({ queryKey: ['get', LIST_PROJECTS_PATH] });
      saveDone.value = true;
    } catch {
      submitError.value = 'プロジェクトを更新できませんでした';
    }
  },
});

/**
 * プロジェクトが差し替わったらフォームとアイコンを引き直す。
 *
 * 一般タブの入力値だけは props から一度コピーして持つので、追従しないと A の
 * 名前と説明を表示したまま B へ保存できてしまう。親の `:key` でも作り直されるが、
 * 各セクションと同じくここだけでも成立させる（`:key` を外した人が
 * この一番重い経路を静かに壊せる状態にしない）。
 */
watch(
  () => props.project.id,
  () => {
    submitError.value = null;
    saveDone.value = false;
    icon.value = props.project.icon_emoji ?? null;
    form.reset({
      name: props.project.name,
      description: props.project.description,
    });
  },
);

const isPending = computed(() => updateMutation.isPending.value);

function onDeleted() {
  void navigate(`/${props.tenantSlug}/my-tasks`);
}
</script>

<template>
  <div class="mx-auto w-full max-w-[1080px]">
    <div class="mb-6">
      <h1 class="mb-1 text-3xl font-bold tracking-tight">プロジェクト設定</h1>
      <p class="text-sm text-muted-foreground">
        {{ project.name }} ·
        <span class="font-mono text-xs">{{ project.key }}</span>
      </p>
    </div>

    <!-- モバイルは縦積み＋ナビ横スクロール、md 以上で横並び -->
    <div class="flex flex-col gap-6 md:flex-row md:items-start md:gap-8">
      <!-- セクションナビ -->
      <nav
        class="flex w-full gap-1 overflow-x-auto md:sticky md:top-2 md:w-[200px] md:shrink-0 md:flex-col md:gap-px"
        aria-label="設定セクション"
      >
        <template v-for="section in sections" :key="section.key">
          <div
            v-if="section.danger"
            class="mx-2 my-2 hidden h-px bg-border md:block"
            aria-hidden="true"
          />
          <button
            type="button"
            class="flex shrink-0 items-center gap-2.5 whitespace-nowrap rounded-md px-2.5 py-1.5 text-left text-sm md:w-full"
            :class="[
              activeSection === section.key ? 'bg-accent font-medium' : 'hover:bg-accent/50',
              section.danger ? 'text-destructive' : '',
            ]"
            :aria-current="activeSection === section.key ? 'true' : undefined"
            @click="activeSection = section.key"
          >
            <component
              :is="section.icon"
              class="size-4"
              :class="section.danger ? '' : 'text-muted-foreground'"
            />
            <span class="flex-1">{{ section.label }}</span>
          </button>
        </template>
      </nav>

      <!-- セクション本体 -->
      <div class="w-full min-w-0 max-w-[640px] flex-1">
        <!-- 一般 -->
        <form v-if="activeSection === 'general'" @submit.prevent="form.handleSubmit">
          <h2 class="mb-6 border-b pb-4 text-xl font-semibold">一般</h2>

          <div class="mb-5 flex items-start gap-4">
            <div class="shrink-0">
              <span class="mb-1.5 block text-sm font-medium">アイコン</span>
              <EmojiIconPicker v-model="icon" />
            </div>
            <div class="min-w-0 flex-1">
              <form.Field name="name">
                <template #default="{ field }">
                  <Field>
                    <FieldLabel :for="field.name">名前</FieldLabel>
                    <Input
                      :id="field.name"
                      placeholder="プロジェクト名"
                      :model-value="field.state.value"
                      @blur="field.handleBlur"
                      @update:model-value="(v) => field.handleChange(String(v))"
                    />
                    <p class="text-xs text-muted-foreground">
                      サイドバーとパンくずに表示されます。
                    </p>
                    <FieldError v-if="field.state.meta.isTouched && field.state.meta.errors.length"
                      >名前は必須です</FieldError
                    >
                  </Field>
                </template>
              </form.Field>
            </div>
          </div>

          <div class="mb-5 max-w-[220px]">
            <Field>
              <FieldLabel for="project-key-readonly">キー</FieldLabel>
              <Input
                id="project-key-readonly"
                class="font-mono"
                :model-value="project.key"
                disabled
                aria-describedby="project-key-fixed-help"
              />
              <p id="project-key-fixed-help" class="text-xs text-muted-foreground">
                タスク番号（例:
                <span class="font-mono">{{ project.key + '-1' }}</span
                >）の基準のため変更できません
              </p>
            </Field>
          </div>

          <div class="mb-5">
            <form.Field name="description">
              <template #default="{ field }">
                <Field>
                  <FieldLabel :for="field.name">説明</FieldLabel>
                  <Textarea
                    :id="field.name"
                    rows="3"
                    placeholder="このプロジェクトは何のためのものですか？"
                    :model-value="field.state.value"
                    @update:model-value="(v) => field.handleChange(String(v))"
                  />
                </Field>
              </template>
            </form.Field>
          </div>

          <p v-if="submitError" role="alert" class="mb-4 text-sm text-destructive">
            {{ submitError }}
          </p>
          <p v-else-if="saveDone" role="status" class="mb-4 text-sm text-muted-foreground">
            変更を保存しました
          </p>

          <div class="border-t pt-4">
            <form.Subscribe>
              <template #default="{ canSubmit, isSubmitting }">
                <Button type="submit" :disabled="!canSubmit || isSubmitting || isPending">
                  {{ isSubmitting ? '保存中…' : '変更を保存' }}
                </Button>
              </template>
            </form.Subscribe>
          </div>
        </form>

        <!-- メンバー -->
        <MembersSection
          v-else-if="activeSection === 'members'"
          :tenant-id="tenantId"
          :project-id="project.id"
        />

        <!-- ワークフロー -->
        <WorkflowStatusesEditor
          v-else-if="activeSection === 'workflow'"
          :tenant-id="tenantId"
          :project-id="project.id"
        />

        <!-- ラベル -->
        <LabelsSection
          v-else-if="activeSection === 'labels'"
          :tenant-id="tenantId"
          :project-id="project.id"
        />

        <!-- カスタムフィールド -->
        <CustomFieldsSection
          v-else-if="activeSection === 'fields'"
          :tenant-id="tenantId"
          :project-id="project.id"
        />

        <!-- 連携 -->
        <IntegrationsSection
          v-else-if="activeSection === 'integrations'"
          :tenant-id="tenantId"
          :project-id="project.id"
        />

        <!-- 削除 -->
        <div v-else-if="activeSection === 'danger'">
          <h2 class="mb-5 border-b pb-4 text-xl font-semibold text-destructive">削除</h2>
          <div class="overflow-hidden rounded-[10px] border border-destructive/30">
            <div class="flex items-center justify-between gap-4 p-4">
              <div>
                <p class="text-sm font-medium">プロジェクトを削除</p>
                <p class="mt-0.5 text-xs text-muted-foreground">
                  このプロジェクトとすべてのタスクを完全に削除します。
                </p>
              </div>
              <Button
                type="button"
                variant="destructive"
                class="shrink-0"
                aria-label="プロジェクトを削除"
                @click="isDeleteOpen = true"
                >削除</Button
              >
            </div>
          </div>
        </div>
      </div>
    </div>

    <DeleteProjectDialog
      :open="isDeleteOpen"
      :tenant-id="tenantId"
      :project="project"
      @update:open="isDeleteOpen = $event"
      @deleted="onDeleted"
    />
  </div>
</template>
