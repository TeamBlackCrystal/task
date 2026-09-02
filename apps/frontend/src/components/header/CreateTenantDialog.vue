<script setup lang="ts">
import { useForm } from '@tanstack/vue-form';
import { type } from 'arktype';
import { ref, watch } from 'vue';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Field, FieldError, FieldGroup, FieldLabel } from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { useTenantStore } from '@/stores/tenant';

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ 'update:open': [open: boolean] }>();
const store = useTenantStore();
const submitError = ref<string | null>(null);
const displayIdEdited = ref(false);

const slugify = (value: string) =>
  value
    .normalize('NFKD')
    .toLowerCase()
    .trim()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 50);

const nonBlankName = type('string').narrow((name) => name.trim().length >= 1);

const schema = type({
  name: nonBlankName,
  display_id: /^[a-z0-9]+(?:-[a-z0-9]+)*$/,
  description: 'string',
  icon_url: 'string',
});

const form = useForm({
  defaultValues: { name: '', display_id: '', description: '', icon_url: '' },
  validators: { onSubmit: schema },
  onSubmit: async ({ value }) => {
    submitError.value = null;
    try {
      const tenant = await store.createTenant({
        name: value.name.trim(),
        display_id: value.display_id,
        ...(value.description.trim() ? { description: value.description.trim() } : {}),
        ...(value.icon_url.trim() ? { icon_url: value.icon_url.trim() } : {}),
      });
      emit('update:open', false);
      form.reset();
      displayIdEdited.value = false;
      window.location.assign(`/${tenant.display_id}/my-tasks`);
    } catch (error) {
      submitError.value = error instanceof Error ? error.message : 'テナントを作成できませんでした';
    }
  },
});

function onOpenChange(open: boolean) {
  emit('update:open', open);
}

watch(
  () => props.open,
  (open) => {
    if (!open) {
      submitError.value = null;
      form.reset();
      displayIdEdited.value = false;
    }
  },
);
</script>

<template>
  <Dialog v-if="open" :open="true" @update:open="onOpenChange">
    <DialogContent class="max-w-lg" :show-close-button="false">
      <DialogHeader>
        <DialogTitle>テナントを作成</DialogTitle>
        <DialogDescription>
          新しいワークスペースの名前と表示IDを入力してください。
        </DialogDescription>
      </DialogHeader>
      <form class="space-y-4" @submit.prevent="form.handleSubmit">
        <FieldGroup>
          <form.Field name="name">
            <template #default="{ field }">
              <Field>
                <FieldLabel :for="field.name">名前</FieldLabel>
                <Input
                  :id="field.name"
                  autofocus
                  :model-value="field.state.value"
                  @blur="field.handleBlur"
                  @update:model-value="
                    (v) => {
                      const name = String(v);
                      field.handleChange(name);
                      if (!displayIdEdited) form.setFieldValue('display_id', slugify(name));
                    }
                  "
                />
                <FieldError v-if="field.state.meta.isTouched && field.state.meta.errors.length"
                  >名前は必須です</FieldError
                >
              </Field>
            </template>
          </form.Field>
          <form.Field name="display_id">
            <template #default="{ field }">
              <Field>
                <FieldLabel :for="field.name">表示ID</FieldLabel>
                <Input
                  :id="field.name"
                  :model-value="field.state.value"
                  aria-describedby="display-id-help"
                  @blur="field.handleBlur"
                  @update:model-value="
                    (v) => {
                      displayIdEdited = true;
                      field.handleChange(String(v).toLowerCase());
                    }
                  "
                />
                <p id="display-id-help" class="text-xs text-muted-foreground">
                  英小文字・数字・ハイフンのみ
                </p>
                <FieldError v-if="field.state.meta.isTouched && field.state.meta.errors.length"
                  >有効な表示IDを入力してください</FieldError
                >
              </Field>
            </template>
          </form.Field>
          <form.Field name="description">
            <template #default="{ field }">
              <Field>
                <FieldLabel :for="field.name">説明（任意）</FieldLabel>
                <Textarea
                  :id="field.name"
                  :model-value="field.state.value"
                  @update:model-value="(v) => field.handleChange(String(v))"
                />
              </Field>
            </template>
          </form.Field>
          <form.Field name="icon_url">
            <template #default="{ field }">
              <Field>
                <FieldLabel :for="field.name">アイコンURL（任意）</FieldLabel>
                <Input
                  :id="field.name"
                  type="url"
                  :model-value="field.state.value"
                  @update:model-value="(v) => field.handleChange(String(v))"
                />
              </Field>
            </template>
          </form.Field>
        </FieldGroup>
        <p v-if="submitError" role="alert" class="text-sm text-destructive">{{ submitError }}</p>
        <DialogFooter>
          <DialogClose as-child>
            <Button type="button" variant="outline">キャンセル</Button>
          </DialogClose>
          <form.Subscribe>
            <template #default="{ canSubmit, isSubmitting }">
              <Button type="submit" :disabled="!canSubmit || isSubmitting">
                {{ isSubmitting ? '作成中…' : '作成' }}
              </Button>
            </template>
          </form.Subscribe>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
</template>
