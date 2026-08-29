<script setup lang="ts">
import { useQueryClient } from '@tanstack/vue-query';
import { useForm } from '@tanstack/vue-form';
import { computed, ref } from 'vue';
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar';
import { Button } from '@/components/ui/button';
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel } from '@/components/ui/field';
import HydrationSafeForm from '@/components/HydrationSafeForm.vue';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { meQueryOptions, useUpdateProfileMutation } from '@/lib/api-vue-query';
import { codePointLength } from '@/lib/code-points';
import { avatarInitials } from '@/lib/initials';
import type { components } from '@/generated/api';

const props = defineProps<{ user: components['schemas']['UserResponse'] }>();

const USERNAME_MIN = 3;
const USERNAME_MAX = 255;
const BIO_MAX = 1000;
const AVATAR_URL_MAX = 2048;

/** backend の `validate_avatar_url` と同じ判定。ずれると保存時まで気づけない。 */
function isHttpsUrl(value: string) {
  const lowered = value.toLowerCase();
  return lowered.startsWith('https://');
}

function validateUsername(raw: string) {
  const length = codePointLength(raw.trim());
  if (length < USERNAME_MIN) return `${USERNAME_MIN}文字以上で入力してください。`;
  if (length > USERNAME_MAX) return `${USERNAME_MAX}文字以内で入力してください。`;
  return undefined;
}

function validateBio(value: string) {
  return codePointLength(value) > BIO_MAX ? `${BIO_MAX}文字以内で入力してください。` : undefined;
}

function validateAvatarUrl(raw: string) {
  // 送信時と同じく trim した値で判定する。貼り付けで前後に空白が入っても、
  // 画面で弾いておいて送信では通る（またはその逆）というずれを作らない。
  const value = raw.trim();
  if (value === '') return undefined;
  if (codePointLength(value) > AVATAR_URL_MAX)
    return `${AVATAR_URL_MAX}文字以内で入力してください。`;
  if (!isHttpsUrl(value)) return 'https:// で始まる URL を入力してください。';
  return undefined;
}

const queryClient = useQueryClient();
const updateProfile = useUpdateProfileMutation();
const submitError = ref<string | null>(null);
const saved = ref(false);
const initialAvatarUrl = props.user.avatar_url ?? '';

const form = useForm({
  defaultValues: {
    username: props.user.username,
    bio: props.user.bio ?? '',
    avatarUrl: initialAvatarUrl,
  },
  onSubmit: async ({ value }) => {
    submitError.value = null;
    saved.value = false;
    const username = value.username.trim();
    const avatarUrl = value.avatarUrl.trim();
    try {
      await updateProfile.mutateAsync({
        body: {
          username,
          bio: value.bio,
          ...(value.avatarUrl === initialAvatarUrl
            ? {}
            : avatarUrl === ''
              ? { clear_avatar_url: true }
              : { avatar_url: avatarUrl }),
        },
      });
      await queryClient.invalidateQueries({ queryKey: meQueryOptions().queryKey });
      saved.value = true;
    } catch (e) {
      const err = e as { response?: { status?: number } };
      submitError.value =
        err.response?.status === 400
          ? '入力内容を確認してください。'
          : '保存できませんでした。時間をおいて再度お試しください。';
    }
  },
});

const avatarFallback = computed(() => avatarInitials(props.user.username));

function resetSubmitFeedback() {
  saved.value = false;
  submitError.value = null;
}
</script>

<template>
  <HydrationSafeForm
    v-slot="{ isHydrated }"
    @submit="() => form.handleSubmit()"
    @input="resetSubmitFeedback"
  >
    <FieldGroup>
      <form.Field
        name="avatarUrl"
        :validators="{
          onBlur: ({ value }) =>
            value === initialAvatarUrl ? undefined : validateAvatarUrl(value),
          onSubmit: ({ value }) =>
            value === initialAvatarUrl ? undefined : validateAvatarUrl(value),
        }"
      >
        <template #default="{ field }">
          <Field>
            <FieldLabel :for="field.name">アバター</FieldLabel>
            <div class="flex items-center gap-4">
              <Avatar class="size-14 rounded-lg">
                <AvatarImage
                  v-if="validateAvatarUrl(field.state.value) === undefined && field.state.value"
                  :src="field.state.value.trim()"
                  :alt="user.username"
                />
                <AvatarFallback class="rounded-lg">{{ avatarFallback }}</AvatarFallback>
              </Avatar>
              <div class="flex flex-1 flex-col gap-2">
                <Input
                  :id="field.name"
                  :name="field.name"
                  type="url"
                  inputmode="url"
                  placeholder="https://example.com/avatar.png"
                  :model-value="field.state.value"
                  @blur="field.handleBlur"
                  @update:model-value="(v) => field.handleChange(String(v))"
                />
                <div>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    :disabled="!field.state.value"
                    @click="
                      field.handleChange('');
                      resetSubmitFeedback();
                    "
                  >
                    削除
                  </Button>
                </div>
              </div>
            </div>
            <FieldDescription>
              画像の URL を指定します。ファイルのアップロードには未対応です。
            </FieldDescription>
            <FieldError class="min-h-[1.25rem]">
              {{ field.state.meta.errors.length ? field.state.meta.errors[0] : '' }}
            </FieldError>
          </Field>
        </template>
      </form.Field>

      <form.Field
        name="username"
        :validators="{
          onBlur: ({ value }) => validateUsername(value),
          onSubmit: ({ value }) => validateUsername(value),
        }"
      >
        <template #default="{ field }">
          <Field>
            <FieldLabel :for="field.name">ユーザー名</FieldLabel>
            <Input
              :id="field.name"
              :name="field.name"
              autocomplete="username"
              :model-value="field.state.value"
              @blur="field.handleBlur"
              @update:model-value="(v) => field.handleChange(String(v))"
            />
            <FieldError class="min-h-[1.25rem]">
              {{ field.state.meta.errors.length ? field.state.meta.errors[0] : '' }}
            </FieldError>
          </Field>
        </template>
      </form.Field>

      <form.Field
        name="bio"
        :validators="{
          onBlur: ({ value }) => validateBio(value),
          onSubmit: ({ value }) => validateBio(value),
        }"
      >
        <template #default="{ field }">
          <Field>
            <FieldLabel :for="field.name">自己紹介</FieldLabel>
            <Textarea
              :id="field.name"
              :name="field.name"
              rows="4"
              :model-value="field.state.value"
              @blur="field.handleBlur"
              @update:model-value="(v) => field.handleChange(String(v))"
            />
            <FieldError class="min-h-[1.25rem]">
              {{ field.state.meta.errors.length ? field.state.meta.errors[0] : '' }}
            </FieldError>
          </Field>
        </template>
      </form.Field>

      <p v-if="submitError" class="text-destructive text-sm">{{ submitError }}</p>
      <p v-else-if="saved" class="text-muted-foreground text-sm">保存しました。</p>

      <form.Subscribe>
        <template #default="{ canSubmit, isSubmitting }">
          <Field>
            <div>
              <Button type="submit" :disabled="!canSubmit || isSubmitting || !isHydrated">
                {{ isSubmitting ? '保存中…' : '変更を保存' }}
              </Button>
            </div>
          </Field>
        </template>
      </form.Subscribe>
    </FieldGroup>
  </HydrationSafeForm>
</template>
