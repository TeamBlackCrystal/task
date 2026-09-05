<script setup lang="ts">
import { PhSmiley, PhTrash, PhUploadSimple } from '@phosphor-icons/vue';

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Separator } from '@/components/ui/separator';

/** デザイン準拠の選択肢。 */
const EMOJI_CHOICES = [
  '🗂️',
  '📦',
  '🚀',
  '🛠️',
  '🧩',
  '📐',
  '💡',
  '🔭',
  '🏢',
  '🏗️',
  '🧪',
  '📊',
  '🎯',
  '🗺️',
  '⚙️',
  '🧭',
  '📎',
  '🖇️',
  '🔧',
  '🪄',
  '🧱',
  '🗃️',
  '📁',
  '🔩',
];

const props = defineProps<{
  /** 画像 URL。空なら絵文字を出す */
  imageUrl: string;
  emoji: string;
  disabled?: boolean;
}>();

const emit = defineEmits<{
  'update:emoji': [value: string];
  'update:imageUrl': [value: string];
}>();
</script>

<template>
  <DropdownMenu>
    <DropdownMenuTrigger as-child>
      <button
        type="button"
        aria-label="テナントアイコンを変更"
        :disabled="props.disabled"
        class="flex size-14 items-center justify-center overflow-hidden rounded-[10px] border bg-secondary p-0"
      >
        <span
          v-if="props.imageUrl"
          class="block size-full bg-cover bg-center"
          :style="{ backgroundImage: `url(${props.imageUrl})` }"
        />
        <span v-else class="text-[28px] leading-none">{{ props.emoji }}</span>
      </button>
    </DropdownMenuTrigger>

    <DropdownMenuContent align="start" class="w-[272px] p-1">
      <label
        class="flex cursor-not-allowed items-center gap-2 rounded px-2 py-1.5 text-sm opacity-50"
      >
        <PhUploadSimple class="size-4 shrink-0 text-muted-foreground" />
        <span class="min-w-0 flex-1">画像をアップロード（準備中）</span>
        <input type="file" class="hidden" disabled />
      </label>

      <p class="px-2 py-1 text-xs text-muted-foreground">
        画像アップロードは専用 API の実装後に利用できます。
      </p>

      <Separator class="my-1" />

      <div class="flex items-center gap-2 px-2 py-1.5">
        <PhSmiley class="size-4 shrink-0 text-muted-foreground" />
        <span class="min-w-0 flex-1 text-xs font-medium text-muted-foreground">絵文字を選択</span>
      </div>
      <div class="grid grid-cols-8 gap-0.5 px-1 pb-1">
        <button
          v-for="choice in EMOJI_CHOICES"
          :key="choice"
          type="button"
          :aria-label="`アイコン ${choice}`"
          :disabled="props.disabled"
          class="h-[30px] rounded-md text-lg leading-none hover:bg-accent"
          :class="!props.imageUrl && choice === props.emoji ? 'bg-secondary' : ''"
          @click="
            emit('update:emoji', choice);
            emit('update:imageUrl', '');
          "
        >
          {{ choice }}
        </button>
      </div>

      <template v-if="props.imageUrl">
        <Separator class="my-1" />
        <button
          type="button"
          :disabled="props.disabled"
          class="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-sm text-destructive hover:bg-accent"
          @click="emit('update:imageUrl', '')"
        >
          <PhTrash class="size-4 shrink-0" />
          <span class="min-w-0 flex-1">画像を削除</span>
        </button>
      </template>
    </DropdownMenuContent>
  </DropdownMenu>
</template>
