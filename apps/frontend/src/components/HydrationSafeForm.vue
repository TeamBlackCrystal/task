<script setup lang="ts">
import { useHydrated } from '@/composables/useHydrated';

const emit = defineEmits<{
  submit: [event: SubmitEvent];
}>();

const isHydrated = useHydrated();

function handleSubmit(event: SubmitEvent) {
  if (!isHydrated.value) {
    event.preventDefault();
    return;
  }

  emit('submit', event);
}

function preventPrehydrationEnter(event: KeyboardEvent) {
  if (!isHydrated.value) {
    event.preventDefault();
  }
}
</script>

<template>
  <!--
    novalidate: ネイティブの制約検証（type="email" など）が submit を先に止めると
    @submit ハンドラーが呼ばれず、フォーム側が出したいエラー表示に到達できない。
    検証はどのフォームも TanStack Form 側に一本化している
  -->
  <form
    novalidate
    :data-hydrated="isHydrated ? 'true' : 'false'"
    :onsubmit.attr="isHydrated ? null : 'return false;'"
    @submit.prevent="handleSubmit"
    @keydown.enter="preventPrehydrationEnter"
  >
    <slot :is-hydrated="isHydrated" />
  </form>
</template>
