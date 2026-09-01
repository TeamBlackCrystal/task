<script setup lang="ts">
import { navigate } from 'vike/client/router';

import { taskDetailHref } from '@/lib/task-display';

const props = defineProps<{
  tenantDisplayId: string;
  projectKey: string;
  seqId: number;
  title: string;
  /** true のとき、素の左クリックはフルページ遷移でなく select emit（分割ビューでの inline 選択）にする */
  inlineSelect?: boolean;
}>();

const emit = defineEmits<{
  select: [seqId: number];
}>();

function navigateToTask(event: MouseEvent) {
  if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey)
    return;
  event.preventDefault();
  if (props.inlineSelect) {
    emit('select', props.seqId);
    return;
  }
  void navigate(taskDetailHref(props.tenantDisplayId, props.projectKey, props.seqId));
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
    @click="navigateToTask"
  >
    {{ title }}
  </a>
</template>
