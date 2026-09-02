<script setup lang="ts">
import { taskDetailHref } from '@/lib/task-display';

const props = defineProps<{
  tenantDisplayId: string;
  projectKey: string;
  seqId: number;
  title: string;
}>();

const emit = defineEmits<{
  select: [seqId: number];
}>();

/**
 * 素の左クリックは呼び出し側に委ね、修飾キー付きは `href`（フルページ）に任せる。
 *
 * 「分割ビューに出すか詳細ページへ送るか」をここで決めない。その判定は画面幅に
 * 依存し、**描画時ではなくクリック時**に読む必要があるため呼び出し側に置く
 * （`select` を受けた側が決める）。真偽値の prop として受け取ると、描画時の値が
 * 固まって古いままになる。
 */
function onPlainClick(event: MouseEvent) {
  if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey)
    return;
  event.preventDefault();
  emit('select', props.seqId);
}
</script>

<template>
  <!--
    以前はここで `after:absolute after:inset-0` を敷き、当たり判定を行全体へ広げていた。
    `<tr>` は WebKit で絶対配置の包含ブロックにならないため、判定が行を飛び越えて
    テーブル全体へ広がり、「どこをタップしても一番下の行が開く」状態になっていた。
    行全体の当たり判定は一覧側（TableRow の click）が持つ。
  -->
  <a
    :href="taskDetailHref(tenantDisplayId, projectKey, seqId)"
    class="truncate text-sm text-primary hover:underline"
    @click="onPlainClick"
  >
    {{ title }}
  </a>
</template>
