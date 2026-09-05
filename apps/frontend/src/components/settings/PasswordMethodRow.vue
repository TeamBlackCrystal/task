<script setup lang="ts">
import { PhLockKey, PhWarning } from '@phosphor-icons/vue';
import { computed, ref } from 'vue';
import { Button } from '@/components/ui/button';
import { Field, FieldError, FieldLabel } from '@/components/ui/field';
import HydrationSafeForm from '@/components/HydrationSafeForm.vue';
import PasswordInput from '@/components/auth/PasswordInput.vue';
import PasswordStrengthBar from '@/components/auth/PasswordStrengthBar.vue';
import { usePasswordStrength } from '@/composables/usePasswordStrength';
import { usePasswordChangeMutation, useSetPasswordMutation } from '@/lib/api-vue-query';
import {
  hasPasswordFormError,
  validatePasswordForm,
  type PasswordFormMode,
} from '@/lib/auth-methods';

const props = defineProps<{ hasPassword: boolean; email: string }>();

const emit = defineEmits<{ set: [] }>();

const mode = computed<PasswordFormMode>(() => (props.hasPassword ? 'change' : 'set'));

const isOpen = ref(false);
const current = ref('');
const next = ref('');
const confirm = ref('');
const touched = ref(false);
const submitError = ref<string | null>(null);

const { strength } = usePasswordStrength(next);

const errors = computed(() =>
  validatePasswordForm(mode.value, {
    current: current.value,
    next: next.value,
    confirm: confirm.value,
  }),
);
/** 入力途中で赤くしないよう、送信を試みるまではエラーを出さない。 */
const shownErrors = computed(() => (touched.value ? errors.value : {}));

const setPassword = useSetPasswordMutation();
const changePassword = usePasswordChangeMutation();
const isSubmitting = computed(() => setPassword.isPending.value || changePassword.isPending.value);

const actionLabel = computed(() => {
  if (isOpen.value) return '閉じる';
  return props.hasPassword ? 'パスワードを変更' : 'パスワードを設定';
});

const submitLabel = computed(() => (props.hasPassword ? 'パスワードを変更' : 'パスワードを設定'));

const warning = computed(() =>
  props.hasPassword
    ? 'パスワードを変更すると、この端末を含むすべてのセッションとパーソナルアクセストークンが失効します。変更後はサインイン画面に移動し、再度サインインが必要です。'
    : 'パスワードを設定すると、OAuth 連携が使えない場合でもメールアドレスとパスワードでサインインできます。現在のセッションは維持されます。',
);

function reset() {
  isOpen.value = false;
  current.value = '';
  next.value = '';
  confirm.value = '';
  touched.value = false;
  submitError.value = null;
}

function toggle() {
  if (isOpen.value) {
    reset();
    return;
  }
  isOpen.value = true;
}

function messageOf(e: unknown): string | undefined {
  return (e as { error?: { message?: string } }).error?.message;
}

async function onSubmit() {
  touched.value = true;
  submitError.value = null;
  if (hasPasswordFormError(errors.value)) return;

  try {
    if (mode.value === 'set') {
      await setPassword.mutateAsync({ body: { password: next.value }, parseAs: 'text' });
      reset();
      emit('set');
      return;
    }

    await changePassword.mutateAsync({
      body: { current_password: current.value, new_password: next.value },
    });
    // セッションも PAT も失効済み。手元のキャッシュを持ち越さないよう、
    // クライアントルーティングではなくフルページ遷移でサインインへ移す。
    window.location.assign('/signin?password_changed=1');
  } catch (e) {
    const message = messageOf(e);
    if (message === 'invalid-current-password') {
      submitError.value = '現在のパスワードが違います。';
    } else if (message === 'password-already-set') {
      submitError.value =
        'すでにパスワードが設定されています。画面を再読み込みしてからやり直してください。';
    } else {
      submitError.value = '保存に失敗しました。時間をおいて再度お試しください。';
    }
  }
}
</script>

<template>
  <div class="flex flex-col">
    <div class="flex flex-wrap items-center gap-3 p-4">
      <span
        class="bg-secondary text-muted-foreground flex size-9 shrink-0 items-center justify-center rounded-lg"
      >
        <PhLockKey class="size-5" />
      </span>
      <div class="min-w-48 flex-1">
        <div class="flex items-center gap-2">
          <span class="text-sm font-medium">パスワード</span>
          <span
            class="inline-flex h-5 items-center rounded-full border px-2 text-xs leading-none font-medium"
            :class="
              hasPassword
                ? 'bg-secondary text-muted-foreground'
                : 'border-amber-500/40 bg-amber-500/10 text-amber-700 dark:text-amber-500'
            "
            >{{ hasPassword ? '設定済み' : '未設定' }}</span
          >
        </div>
        <p class="text-muted-foreground text-xs">
          {{ hasPassword ? email : 'OAuth 連携のみでサインインしています' }}
        </p>
      </div>
      <Button type="button" variant="outline" size="sm" @click="toggle">
        {{ actionLabel }}
      </Button>
    </div>

    <div v-if="isOpen" class="px-4 pb-4 md:pl-16">
      <div class="bg-secondary mb-4 flex items-start gap-2 rounded-md border p-3">
        <PhWarning class="text-muted-foreground mt-0.5 size-4 shrink-0" />
        <p class="text-sm">{{ warning }}</p>
      </div>

      <HydrationSafeForm
        v-slot="{ isHydrated }"
        class="flex max-w-md flex-col gap-4"
        @submit="onSubmit"
      >
        <Field v-if="mode === 'change'">
          <FieldLabel for="password-current">現在のパスワード</FieldLabel>
          <PasswordInput id="password-current" v-model="current" autocomplete="current-password" />
          <FieldError v-if="shownErrors.current">{{ shownErrors.current }}</FieldError>
        </Field>

        <Field>
          <FieldLabel for="password-next">
            {{ mode === 'set' ? 'パスワード' : '新しいパスワード' }}
          </FieldLabel>
          <PasswordInput id="password-next" v-model="next" autocomplete="new-password" />
          <PasswordStrengthBar v-if="next.length > 0" :strength="strength" />
          <FieldError v-if="shownErrors.next">{{ shownErrors.next }}</FieldError>
        </Field>

        <Field>
          <FieldLabel for="password-confirm">
            {{ mode === 'set' ? 'パスワード（確認）' : '新しいパスワード（確認）' }}
          </FieldLabel>
          <PasswordInput id="password-confirm" v-model="confirm" autocomplete="new-password" />
          <FieldError v-if="shownErrors.confirm">{{ shownErrors.confirm }}</FieldError>
        </Field>

        <p v-if="submitError" role="alert" class="text-destructive text-sm">{{ submitError }}</p>

        <div class="flex gap-2">
          <Button type="submit" :disabled="isSubmitting || !isHydrated">
            {{ isSubmitting ? '保存中…' : submitLabel }}
          </Button>
          <Button type="button" variant="outline" @click="reset">キャンセル</Button>
        </div>
      </HydrationSafeForm>
    </div>
  </div>
</template>
