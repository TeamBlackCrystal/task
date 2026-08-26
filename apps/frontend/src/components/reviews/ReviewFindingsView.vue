<script setup lang="ts">
import { useQueryClient } from '@tanstack/vue-query';
import { PhCheckCircle, PhPlus, PhWarningCircle } from '@phosphor-icons/vue';
import { computed, ref, watch } from 'vue';
import { Button } from '@/components/ui/button';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import ReviewRoundComposer from '@/components/reviews/ReviewRoundComposer.vue';
import {
  REVIEWED_PRS_PATH,
  REVIEWS_PATH,
  REVIEW_FINDINGS_PATH,
  REVIEW_SUMMARY_PATH,
  useReviewFindingsQuery,
  useReviewRoundsQuery,
  useReviewSummaryQuery,
  useReviewedPullRequestsQuery,
  useUpdateFindingStateMutation,
} from '@/lib/api-vue-query';
import {
  SEVERITIES,
  SEVERITY_LABELS,
  STATES,
  STATE_LABELS,
  findingActions,
  findingLocation,
  sortFindings,
  summaryRows,
  type FindingSeverity,
  type FindingState,
  type Review,
  type ReviewFinding,
} from '@/lib/review-findings';

const props = defineProps<{
  tenantId: string;
  tenantSlug: string;
  projectId: string;
  projectKey: string;
  /** 現在の利用者。自分の修正を自分で確認できない判定に使う */
  viewerId: string;
  /** 初期表示する PR（要約コメントのリンクから来たとき） */
  initialPr?: number | null;
}>();

const queryClient = useQueryClient();

const selectedPr = ref<number | null>(props.initialPr ?? null);
const roundFilter = ref<number | null>(null);
const severityFilter = ref<FindingSeverity | null>(null);
const stateFilter = ref<FindingState | null>(null);
const isComposerOpen = ref(false);
const transitionError = ref<string | null>(null);

const prsQuery = useReviewedPullRequestsQuery(props.tenantId, props.projectId);
const roundsQuery = useReviewRoundsQuery(props.tenantId, props.projectId, selectedPr);
const findingsQuery = useReviewFindingsQuery(props.tenantId, props.projectId, selectedPr);
const summaryQuery = useReviewSummaryQuery(props.tenantId, props.projectId, selectedPr);
const updateState = useUpdateFindingStateMutation();

const pullRequests = computed(() => prsQuery.data.value ?? []);
const rounds = computed(() => roundsQuery.data.value ?? []);

/**
 * 指摘 → それを出したラウンドの作成者。取り下げを出してよいかの判定に使う。
 *
 * ラウンド一覧が未取得のうちは分からないので、その間は取り下げを出さない
 * （押せるのに 403 になるボタンを出さないため）。
 */
const findingAuthorId = (finding: ReviewFinding): string | null =>
  rounds.value.find((round: Review) => round.id === finding.review_id)?.reviewer.id ?? null;
const summary = computed(() => summaryQuery.data.value ?? null);

/** 初期 PR の指定が無ければ、最後にレビューされた PR を開く。 */
watch(pullRequests, (list) => {
  if (selectedPr.value === null && list.length > 0) {
    selectedPr.value = list[0].pr_number;
  }
});

/** 絞り込みは画面側で適用する（API は PR 単位で全件返す）。 */
const findings = computed(() => {
  const all = sortFindings(findingsQuery.data.value ?? []);
  return all.filter(
    (finding) =>
      (roundFilter.value === null || finding.round === roundFilter.value) &&
      (severityFilter.value === null || finding.severity === severityFilter.value) &&
      (stateFilter.value === null || finding.state === stateFilter.value),
  );
});

const hasFilters = computed(
  () => roundFilter.value !== null || severityFilter.value !== null || stateFilter.value !== null,
);

function clearFilters() {
  roundFilter.value = null;
  severityFilter.value = null;
  stateFilter.value = null;
}

function selectPr(pr: number) {
  selectedPr.value = pr;
  clearFilters();
  isComposerOpen.value = false;
  transitionError.value = null;
}

async function invalidateAll() {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: ['get', REVIEWED_PRS_PATH] }),
    queryClient.invalidateQueries({ queryKey: ['get', REVIEWS_PATH] }),
    queryClient.invalidateQueries({ queryKey: ['get', REVIEW_FINDINGS_PATH] }),
    queryClient.invalidateQueries({ queryKey: ['get', REVIEW_SUMMARY_PATH] }),
  ]);
}

/**
 * サーバーが本文に入れた理由。読ませる意味のあるものだけ返す。
 *
 * 共通のエラー本文は `conflict` / `forbidden` のようなスラグで、そのまま出しても
 * 利用者は何も判断できない。理由付きの 409（High の繰り延べなど）だけを拾う。
 */
function serverReason(message: string | undefined): string | null {
  if (!message || /^[a-z][a-z-]*$/.test(message)) return null;
  return message;
}

async function transition(finding: ReviewFinding, to: FindingState) {
  transitionError.value = null;
  try {
    await updateState.mutateAsync({
      params: {
        path: { tenant_id: props.tenantId, project_id: props.projectId, id: finding.id },
      },
      body: { state: to, note: null },
    });
    await invalidateAll();
  } catch (e) {
    const err = e as { response?: { status?: number }; error?: { message?: string } };
    const status = err.response?.status;
    transitionError.value =
      serverReason(err.error?.message) ??
      (status === 403
        ? 'この操作はレビュー側だけが行えます（自分の修正は自分で確認できません）。'
        : status === 409
          ? 'いまの状態からは行えない操作です。画面を再読み込みしてください。'
          : '状態を更新できませんでした。');
    // 失敗した操作の結果が画面に残らないよう、正しい状態を取り直す
    await invalidateAll();
  }
}

async function onRoundCreated() {
  isComposerOpen.value = false;
  await invalidateAll();
}
</script>

<template>
  <div class="mx-auto flex w-full max-w-[1080px] flex-col gap-6">
    <div class="flex flex-col gap-1">
      <h1 class="text-2xl font-bold tracking-tight">レビュー指摘</h1>
      <p class="text-muted-foreground text-sm">
        指摘の一覧と状態はここが権威です。GitHub の PR には要約コメントが 1 本だけ置かれます。
      </p>
    </div>

    <p v-if="prsQuery.isPending.value" class="text-muted-foreground text-sm">読み込み中…</p>
    <p v-else-if="prsQuery.isError.value" role="alert" class="text-destructive text-sm">
      レビューを読み込めませんでした
    </p>

    <template v-else>
      <div class="flex flex-col gap-6 md:flex-row md:items-start">
        <!-- PR 一覧 -->
        <nav
          class="flex w-full shrink-0 flex-col gap-1 md:w-[280px]"
          aria-label="レビューのある PR"
        >
          <p v-if="pullRequests.length === 0" class="text-muted-foreground text-sm">
            レビューはまだありません。
          </p>
          <button
            v-for="pr in pullRequests"
            :key="pr.pr_number"
            type="button"
            class="flex flex-col gap-1 rounded-md border px-3 py-2 text-left text-sm"
            :class="
              pr.pr_number === selectedPr
                ? 'bg-accent border-accent-foreground/20'
                : 'hover:bg-accent/50'
            "
            :aria-current="pr.pr_number === selectedPr ? 'true' : undefined"
            @click="selectPr(pr.pr_number)"
          >
            <span class="flex items-center gap-2">
              <span class="font-mono text-xs">#{{ pr.pr_number }}</span>
              <span
                class="rounded-full px-2 py-0.5 text-xs"
                :class="
                  pr.mergeable
                    ? 'bg-green-600/10 text-green-700 dark:text-green-400'
                    : 'bg-destructive/10 text-destructive'
                "
              >
                {{ pr.mergeable ? 'マージ可' : `${pr.blocking} 件が未解決` }}
              </span>
            </span>
            <span class="truncate font-medium">{{ pr.pr_title ?? `PR #${pr.pr_number}` }}</span>
            <span class="text-muted-foreground text-xs">
              <template v-if="pr.pr_author">{{ pr.pr_author }} · </template>R{{ pr.rounds }} ·
              未解決 {{ pr.unresolved }}
            </span>
          </button>
        </nav>

        <!-- 指摘 -->
        <div class="flex min-w-0 flex-1 flex-col gap-4">
          <p v-if="selectedPr === null" class="text-muted-foreground text-sm">
            PR を選ぶと指摘が表示されます。
          </p>

          <template v-else>
            <!-- マージ判定 -->
            <div
              v-if="summary"
              class="flex items-center gap-3 rounded-lg border p-4"
              :class="
                summary.mergeable
                  ? 'border-green-600/40 bg-green-600/5'
                  : 'border-destructive/40 bg-destructive/5'
              "
              data-testid="merge-gate"
            >
              <component
                :is="summary.mergeable ? PhCheckCircle : PhWarningCircle"
                class="size-5 shrink-0"
                :class="
                  summary.mergeable ? 'text-green-700 dark:text-green-400' : 'text-destructive'
                "
              />
              <div class="flex flex-col">
                <span class="text-sm font-medium">
                  {{ summary.mergeable ? 'マージ可' : `マージ不可（${summary.blocking} 件）` }}
                </span>
                <span class="text-muted-foreground text-xs">
                  {{
                    summary.mergeable
                      ? 'High / Medium の未解決はありません'
                      : 'High / Medium が未解決です。Low / Nit は繰り延べできます'
                  }}
                </span>
              </div>
              <div class="ml-auto flex flex-wrap justify-end gap-1">
                <span
                  v-for="row in summaryRows(summary)"
                  :key="`${row.severity}-${row.state}`"
                  class="text-muted-foreground rounded-md border px-2 py-0.5 text-xs"
                >
                  {{ SEVERITY_LABELS[row.severity] }} / {{ STATE_LABELS[row.state] }}:
                  {{ row.count }}
                </span>
              </div>
            </div>

            <!-- 絞り込みと起票 -->
            <div class="flex flex-wrap items-end gap-2">
              <div class="w-[110px]">
                <label for="filter-round" class="mb-1.5 block text-xs font-medium">Round</label>
                <Select
                  :model-value="roundFilter === null ? 'all' : String(roundFilter)"
                  @update:model-value="
                    (v) => (roundFilter = v === 'all' || v == null ? null : Number(v))
                  "
                >
                  <SelectTrigger id="filter-round" size="sm" class="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="all">すべて</SelectItem>
                    <SelectItem
                      v-for="round in rounds"
                      :key="round.id"
                      :value="String(round.round)"
                    >
                      R{{ round.round }}
                    </SelectItem>
                  </SelectContent>
                </Select>
              </div>
              <div class="w-[130px]">
                <label for="filter-severity" class="mb-1.5 block text-xs font-medium">重大度</label>
                <Select
                  :model-value="severityFilter ?? 'all'"
                  @update:model-value="
                    (v) =>
                      (severityFilter = v === 'all' || v == null ? null : (v as FindingSeverity))
                  "
                >
                  <SelectTrigger id="filter-severity" size="sm" class="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="all">すべて</SelectItem>
                    <SelectItem v-for="severity in SEVERITIES" :key="severity" :value="severity">
                      {{ SEVERITY_LABELS[severity] }}
                    </SelectItem>
                  </SelectContent>
                </Select>
              </div>
              <div class="w-[130px]">
                <label for="filter-state" class="mb-1.5 block text-xs font-medium">状態</label>
                <Select
                  :model-value="stateFilter ?? 'all'"
                  @update:model-value="
                    (v) => (stateFilter = v === 'all' || v == null ? null : (v as FindingState))
                  "
                >
                  <SelectTrigger id="filter-state" size="sm" class="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="all">すべて</SelectItem>
                    <SelectItem v-for="state in STATES" :key="state" :value="state">
                      {{ STATE_LABELS[state] }}
                    </SelectItem>
                  </SelectContent>
                </Select>
              </div>
              <Button
                v-if="hasFilters"
                type="button"
                variant="ghost"
                size="sm"
                @click="clearFilters"
              >
                絞り込み解除
              </Button>
              <Button
                v-if="!isComposerOpen"
                type="button"
                size="sm"
                class="ml-auto"
                @click="isComposerOpen = true"
              >
                <PhPlus class="size-4" />
                指摘を起票
              </Button>
            </div>

            <ReviewRoundComposer
              v-if="isComposerOpen"
              :tenant-id="tenantId"
              :project-id="projectId"
              :pr-number="selectedPr"
              :next-round="rounds.length + 1"
              @created="onRoundCreated"
              @cancel="isComposerOpen = false"
            />

            <p v-if="transitionError" role="alert" class="text-destructive text-sm">
              {{ transitionError }}
            </p>

            <p v-if="findingsQuery.isPending.value" class="text-muted-foreground text-sm">
              読み込み中…
            </p>
            <p v-else-if="findings.length === 0" class="text-muted-foreground text-sm">
              {{ hasFilters ? '条件に合う指摘はありません。' : '指摘はありません。' }}
            </p>

            <ul v-else class="flex flex-col gap-3" data-testid="finding-list">
              <li v-for="finding in findings" :key="finding.id" class="rounded-lg border p-4">
                <div class="flex flex-wrap items-center gap-2">
                  <span
                    class="rounded-md border px-2 py-0.5 text-xs font-medium"
                    :class="
                      finding.severity === 'high' || finding.severity === 'medium'
                        ? 'border-destructive/40 text-destructive'
                        : 'text-muted-foreground'
                    "
                  >
                    {{ SEVERITY_LABELS[finding.severity] }}
                  </span>
                  <span class="text-muted-foreground rounded-md border px-2 py-0.5 text-xs">
                    {{ STATE_LABELS[finding.state] }}
                  </span>
                  <span class="text-muted-foreground text-xs">R{{ finding.round }}</span>
                  <span class="min-w-0 flex-1 truncate font-medium">{{ finding.title }}</span>
                </div>

                <p
                  v-if="findingLocation(finding)"
                  class="text-muted-foreground mt-1 font-mono text-xs"
                >
                  {{ findingLocation(finding) }}
                </p>
                <p class="mt-2 text-sm whitespace-pre-wrap">{{ finding.body }}</p>

                <div class="mt-3 flex flex-wrap items-center gap-2">
                  <Button
                    v-for="action in findingActions(finding, viewerId, findingAuthorId(finding))"
                    :key="action.to"
                    type="button"
                    size="sm"
                    :variant="action.to === 'verified' ? 'default' : 'outline'"
                    :disabled="action.disabledReason !== null || updateState.isPending.value"
                    :title="action.disabledReason ?? undefined"
                    @click="transition(finding, action.to)"
                  >
                    {{ action.label }}
                  </Button>
                  <span
                    v-if="
                      findingActions(finding, viewerId, findingAuthorId(finding)).some(
                        (a) => a.disabledReason,
                      )
                    "
                    class="text-muted-foreground text-xs"
                  >
                    修正者と確認者は別の人である必要があります
                  </span>
                </div>

                <details v-if="finding.transitions.length > 0" class="mt-3">
                  <summary class="text-muted-foreground cursor-pointer text-xs">遷移履歴</summary>
                  <ul class="mt-2 flex flex-col gap-1">
                    <li
                      v-for="history in finding.transitions"
                      :key="history.id"
                      class="text-muted-foreground text-xs"
                    >
                      {{ history.actor.username }} ·
                      {{
                        history.from_state
                          ? `${STATE_LABELS[history.from_state]} → ${STATE_LABELS[history.to_state]}`
                          : `${STATE_LABELS[history.to_state]} として登録`
                      }}
                      <template v-if="history.note"> · {{ history.note }}</template>
                    </li>
                  </ul>
                </details>
              </li>
            </ul>
          </template>
        </div>
      </div>
    </template>
  </div>
</template>
