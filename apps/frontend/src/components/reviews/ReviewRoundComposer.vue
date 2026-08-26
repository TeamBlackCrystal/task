<script setup lang="ts">
import { PhTrash } from '@phosphor-icons/vue';
import { computed, ref } from 'vue';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Textarea } from '@/components/ui/textarea';
import { useCreateReviewMutation } from '@/lib/api-vue-query';
import { SEVERITIES, SEVERITY_LABELS, type FindingSeverity } from '@/lib/review-findings';

const props = defineProps<{
  tenantId: string;
  projectId: string;
  prNumber: number;
  /** これから作るラウンドの番号（表示だけに使う。採番はサーバー） */
  nextRound: number;
}>();

const emit = defineEmits<{ created: []; cancel: [] }>();

type StagedFinding = {
  severity: FindingSeverity;
  title: string;
  body: string;
  file: string;
  line: string;
};

const createReview = useCreateReviewMutation();

const headSha = ref('');
const summary = ref('');
/**
 * 下書き。**確定するまでサーバーには何も作らない**（ラウンドは確定時に
 * 指摘ごと一括作成し、確定後は追記できない。仕様 §3）。
 */
const staged = ref<StagedFinding[]>([]);
const submitError = ref<string | null>(null);

const draft = ref<StagedFinding>(emptyDraft());
const draftError = ref<string | null>(null);

function emptyDraft(): StagedFinding {
  return { severity: 'medium', title: '', body: '', file: '', line: '' };
}

const canCommit = computed(() => headSha.value.trim().length > 0);

function addToDraft() {
  draftError.value = null;
  if (draft.value.title.trim() === '' || draft.value.body.trim() === '') {
    draftError.value = 'タイトルと本文を入力してください。';
    return;
  }
  if (draft.value.line.trim() !== '' && !/^\d+$/.test(draft.value.line.trim())) {
    draftError.value = '行番号は数値で入力してください。';
    return;
  }
  staged.value = [...staged.value, { ...draft.value }];
  draft.value = emptyDraft();
}

function removeStaged(index: number) {
  staged.value = staged.value.filter((_, i) => i !== index);
}

async function commit() {
  if (!canCommit.value) return;
  submitError.value = null;
  try {
    await createReview.mutateAsync({
      params: { path: { tenant_id: props.tenantId, project_id: props.projectId } },
      body: {
        pr_number: props.prNumber,
        head_sha: headSha.value.trim(),
        summary: summary.value,
        findings: staged.value.map((finding) => ({
          severity: finding.severity,
          title: finding.title.trim(),
          body: finding.body,
          file: finding.file.trim() === '' ? null : finding.file.trim(),
          line: finding.line.trim() === '' ? null : Number(finding.line.trim()),
        })),
      },
    });
    emit('created');
  } catch (e) {
    const status = (e as { response?: { status?: number } }).response?.status;
    submitError.value =
      status === 403
        ? 'このプロジェクトにレビューを起票する権限がありません。'
        : status === 400
          ? '入力内容を確認してください。'
          : 'ラウンドを確定できませんでした。';
  }
}
</script>

<template>
  <div class="flex flex-col gap-4 rounded-lg border p-4" data-testid="round-composer">
    <div class="flex flex-col gap-0.5">
      <h2 class="text-base font-semibold">新しいラウンド（R{{ nextRound }}）を起票</h2>
      <p class="text-muted-foreground text-xs">下書き · 確定するまでサーバーには作られません</p>
    </div>

    <div class="flex flex-col gap-1.5">
      <label for="composer-head" class="text-sm font-medium">レビュー対象の head SHA</label>
      <Input
        id="composer-head"
        v-model="headSha"
        placeholder="60cdd7795f94fa4e4148ce996c2efb4c363e3f5e"
        class="font-mono"
      />
    </div>

    <div class="flex flex-col gap-1.5">
      <label for="composer-summary" class="text-sm font-medium">総評</label>
      <Textarea
        id="composer-summary"
        v-model="summary"
        rows="2"
        placeholder="具体的な不具合は見つからなかった"
      />
    </div>

    <!-- 下書きに積んだ指摘 -->
    <div v-if="staged.length > 0" class="flex flex-col gap-2">
      <p class="text-sm font-medium">下書きの指摘 {{ staged.length }} 件</p>
      <ul class="divide-y rounded-md border" data-testid="staged-list">
        <li
          v-for="(finding, index) in staged"
          :key="`${finding.title}-${index}`"
          class="flex items-center gap-2 px-3 py-2"
        >
          <span class="text-muted-foreground rounded-md border px-2 py-0.5 text-xs">
            {{ SEVERITY_LABELS[finding.severity] }}
          </span>
          <span class="min-w-0 flex-1 truncate text-sm">{{ finding.title }}</span>
          <span v-if="finding.file" class="text-muted-foreground font-mono text-xs">
            {{ finding.file }}{{ finding.line ? `:${finding.line}` : '' }}
          </span>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            :aria-label="`下書きから「${finding.title}」を外す`"
            @click="removeStaged(index)"
          >
            <PhTrash class="size-4" />
          </Button>
        </li>
      </ul>
    </div>

    <!-- 指摘の入力 -->
    <div class="flex flex-col gap-2 rounded-md border p-3">
      <div class="flex flex-wrap gap-2">
        <div class="w-[130px]">
          <label for="draft-severity" class="mb-1.5 block text-xs font-medium">重大度</label>
          <Select
            :model-value="draft.severity"
            @update:model-value="(v) => (draft.severity = v as FindingSeverity)"
          >
            <SelectTrigger id="draft-severity" size="sm" class="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem v-for="severity in SEVERITIES" :key="severity" :value="severity">
                {{ SEVERITY_LABELS[severity] }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>
        <div class="min-w-[200px] flex-1">
          <label for="draft-title" class="mb-1.5 block text-xs font-medium">タイトル</label>
          <Input id="draft-title" v-model="draft.title" placeholder="1 行の要約" />
        </div>
      </div>

      <div class="flex flex-wrap gap-2">
        <div class="min-w-[200px] flex-1">
          <label for="draft-file" class="mb-1.5 block text-xs font-medium">ファイル（任意）</label>
          <Input id="draft-file" v-model="draft.file" placeholder="src/App.vue" class="font-mono" />
        </div>
        <div class="w-[110px]">
          <label for="draft-line" class="mb-1.5 block text-xs font-medium">行（任意）</label>
          <Input id="draft-line" v-model="draft.line" inputmode="numeric" placeholder="42" />
        </div>
      </div>

      <div>
        <label for="draft-body" class="mb-1.5 block text-xs font-medium">本文</label>
        <Textarea id="draft-body" v-model="draft.body" rows="3" placeholder="再現条件と根拠" />
      </div>

      <p v-if="draftError" role="alert" class="text-destructive text-sm">{{ draftError }}</p>

      <div>
        <Button type="button" variant="outline" size="sm" @click="addToDraft">下書きに追加</Button>
      </div>
    </div>

    <p class="text-muted-foreground text-xs">
      {{
        staged.length === 0
          ? '指摘 0 件でも確定できます（総評だけのラウンドとして記録）'
          : `確定すると ${staged.length} 件が Open として一括作成され、R${nextRound} は以後追記できません`
      }}
    </p>

    <p v-if="submitError" role="alert" class="text-destructive text-sm">{{ submitError }}</p>

    <div class="flex gap-2">
      <Button
        type="button"
        :disabled="!canCommit || createReview.isPending.value"
        :title="canCommit ? undefined : 'head SHA を入力してください'"
        @click="commit"
      >
        {{ createReview.isPending.value ? '確定中…' : `確定（${staged.length} 件）` }}
      </Button>
      <Button type="button" variant="outline" @click="emit('cancel')">キャンセル</Button>
    </div>
  </div>
</template>
