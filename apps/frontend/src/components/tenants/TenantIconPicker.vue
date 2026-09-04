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
}>();

const emit = defineEmits<{
  'update:emoji': [value: string];
  'update:imageUrl': [value: string];
}>();

/** 選んだ画像はその場では data URL で持ち、保存時に本体へ渡す。 */
function onUpload(event: Event) {
  const file = (event.target as HTMLInputElement).files?.[0];
  if (!file) return;
  const reader = new FileReader();
  reader.onload = () => emit('update:imageUrl', String(reader.result));
  reader.readAsDataURL(file);
}
</script>

<template>
  <DropdownMenu>
    <DropdownMenuTrigger as-child>
      <button
        type="button"
        aria-label="テナントアイコンを変更"
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
        class="flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-sm hover:bg-accent"
      >
        <PhUploadSimple class="size-4 shrink-0 text-muted-foreground" />
        <span class="min-w-0 flex-1">画像をアップロード</span>
        <input type="file" accept="image/*" class="hidden" @change="onUpload" />
      </label>

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
