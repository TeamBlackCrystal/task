<!-- Hallmark · pre-emit critique: P5 H4 E4 S4 R5 V4 -->
<script setup lang="ts">
import { computed } from 'vue';
import { PhArrowDown, PhArrowUp, PhCaretDown, PhCheck, PhX } from '@phosphor-icons/vue';
import type { AcceptableValue } from 'reka-ui';

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import {
  activeTaskListSort,
  type TaskListSortColumnOption,
  type TaskListSortingState,
} from '@/components/tasks/task-list-sort';

const props = defineProps<{
  column: TaskListSortColumnOption;
  sorting: TaskListSortingState;
  disabled?: boolean;
}>();

const emit = defineEmits<{
  'update:sorting': [sorting: TaskListSortingState];
}>();

const activeSort = computed(() => activeTaskListSort(props.sorting));
const direction = computed<'asc' | 'desc' | null>(() => {
  if (activeSort.value?.id !== props.column.id) return null;
  return activeSort.value.desc ? 'desc' : 'asc';
});
const triggerLabel = computed(() => {
  const current =
    direction.value === 'asc'
      ? props.column.ascendingLabel
      : direction.value === 'desc'
        ? props.column.descendingLabel
        : null;
  return current
    ? `${props.column.label}を並べ替え、現在は${current}`
    : `${props.column.label}を並べ替え`;
});

function setDirection(value: AcceptableValue) {
  if (value !== 'asc' && value !== 'desc') return;
  emit('update:sorting', [{ id: props.column.id, desc: value === 'desc' }]);
}

function clearSorting() {
  if (direction.value) emit('update:sorting', []);
}

function reverseDirection() {
  if (!direction.value) return;
  setDirection(direction.value === 'asc' ? 'desc' : 'asc');
}
</script>

<template>
  <div class="flex h-8 min-w-0 items-center">
    <DropdownMenu>
      <DropdownMenuTrigger as-child>
        <button
          type="button"
          class="group/sort flex h-8 min-w-0 flex-1 items-center gap-1 rounded-sm px-2 text-left text-xs font-medium whitespace-nowrap text-muted-foreground transition-colors hover:bg-muted/70 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-1 active:bg-muted/90 disabled:cursor-not-allowed disabled:opacity-50 data-[state=open]:bg-muted data-[state=open]:text-foreground"
          :aria-label="triggerLabel"
          :disabled="disabled"
        >
          <span>{{ column.label }}</span>
          <PhCaretDown
            v-if="!direction"
            class="size-3 opacity-45 group-hover/sort:opacity-80 group-focus-visible/sort:opacity-80"
            aria-hidden="true"
          />
        </button>
      </DropdownMenuTrigger>

      <DropdownMenuContent align="start" class="w-56">
        <DropdownMenuRadioGroup :model-value="direction ?? ''" @update:model-value="setDirection">
          <DropdownMenuRadioItem value="asc">
            <template #indicator-icon>
              <PhCheck class="size-3.5" aria-hidden="true" />
            </template>
            <PhArrowUp class="size-4" aria-hidden="true" />
            {{ column.ascendingLabel }}
          </DropdownMenuRadioItem>
          <DropdownMenuRadioItem value="desc">
            <template #indicator-icon>
              <PhCheck class="size-3.5" aria-hidden="true" />
            </template>
            <PhArrowDown class="size-4" aria-hidden="true" />
            {{ column.descendingLabel }}
          </DropdownMenuRadioItem>
        </DropdownMenuRadioGroup>
        <DropdownMenuSeparator />
        <DropdownMenuItem :disabled="!direction" @select="clearSorting">
          <PhX class="size-4" aria-hidden="true" />
          並べ替えをクリア
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>

    <button
      v-if="direction"
      type="button"
      class="mr-1 flex h-7 w-7 shrink-0 items-center justify-center rounded-sm text-muted-foreground transition-colors hover:bg-muted/70 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-1 active:bg-muted/90 disabled:cursor-not-allowed disabled:opacity-50"
      :aria-label="`${column.label}を${direction === 'asc' ? column.descendingLabel : column.ascendingLabel}に反転`"
      :disabled="disabled"
      @click="reverseDirection"
    >
      <PhArrowUp v-if="direction === 'asc'" class="size-3.5" aria-hidden="true" />
      <PhArrowDown v-else class="size-3.5" aria-hidden="true" />
    </button>
  </div>
</template>
