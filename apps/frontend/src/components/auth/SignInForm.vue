<script setup lang="ts">
import { useQueryClient } from '@tanstack/vue-query';
import { useForm } from '@tanstack/vue-form';
import { type } from 'arktype';
import { onMounted, ref } from 'vue';
import EmailNotVerified from '@/components/auth/EmailNotVerified.vue';
import OAuthButtons from '@/components/auth/OAuthButtons.vue';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel } from '@/components/ui/field';
import HydrationSafeForm from '@/components/HydrationSafeForm.vue';
import PasswordInput from '@/components/auth/PasswordInput.vue';
import { Input } from '@/components/ui/input';
import { meQueryOptions, useLoginMutation, useLogoutMutation } from '@/lib/api-vue-query';
import { arkMessage } from '@/lib/auth-validation';

/**
 * パスワード変更はすべてのセッションと PAT を失効させるので、設定画面から
 * `?password_changed=1` 付きでここへ戻ってくる。なぜサインアウトされたのかを伝える。
 */
const passwordChanged = ref(false);

onMounted(() => {
  passwordChanged.value = new URLSearchParams(window.location.search).has('password_changed');
});

const schema = type({
  email: 'string.email',
  password: 'string >= 8',
});

const queryClient = useQueryClient();
const loginMutation = useLoginMutation();
const logoutMutation = useLogoutMutation();
const submitError = ref<string | null>(null);
const unverifiedEmail = ref<string | null>(null);
const submitAttempted = ref(false);

const form = useForm({
  defaultValues: { email: '', password: '' },
  validators: { onSubmit: schema },
  // canSubmit が false だと 1 回目の送信が握り潰される（form-core の _handleSubmit が
  // submissionAttempts <= 1 のとき早期 return する）。パスワードは onBlur 検証で、
  // 直した値を change で検証しても onBlur 由来のエラーは残るため、フィールド内から
  // Enter を押すと無反応になっていた。送信時は全フィールドが submit 起因で再検証され
  // （onChange / onBlur / onSubmit がまとめて走る）古いエラーも消えるので、
  // canSubmit で入口を塞ぐ必要はない
  canSubmitWhenInvalid: true,
  onSubmit: async ({ value }) => {
    submitError.value = null;
    unverifiedEmail.value = null;
    try {
      const result = await loginMutation.mutateAsync({
        body: {
          email: value.email,
          password: value.password,
        },
      });

      if (result && typeof result === 'object' && 'requires_2fa' in result) {
        try {
          await logoutMutation.mutateAsync({} as never);
        } catch {
          // logout failure: still show the same unsupported-2FA message
        }
        submitError.value = '二要素認証は現在サポートされていません。';
        return;
      }

      await queryClient.invalidateQueries({ queryKey: meQueryOptions().queryKey });
      window.location.assign('/');
    } catch (e) {
      // 403 はメール未認証以外（CSRF 拒否・凍結など）でも返るため、
      // ステータスだけでなくエラーボディの message で判定する
      const err = e as { response?: { status?: number }; error?: { message?: string } };
      if (err.response?.status === 403 && err.error?.message === 'email-not-verified') {
        unverifiedEmail.value = value.email;
      } else {
        submitError.value = 'メールアドレスまたはパスワードが正しくありません。';
      }
    }
  },
});

function handleSubmit() {
  submitAttempted.value = true;
  return form.handleSubmit();
}
</script>

<template>
  <EmailNotVerified
    v-if="unverifiedEmail"
    :email="unverifiedEmail"
    back-href="/signin"
    reset-href="/auth/reset-password"
  />
  <div v-else class="flex flex-col gap-6">
    <Card class="overflow-hidden p-0">
      <CardContent class="grid p-0 md:grid-cols-2">
        <HydrationSafeForm v-slot="{ isHydrated }" class="p-6 md:p-8" @submit="handleSubmit">
          <FieldGroup>
            <div class="flex flex-col items-center gap-2 text-center">
              <h1 class="text-2xl font-bold">おかえりなさい</h1>
              <p class="text-muted-foreground text-sm text-balance">
                メールアドレスを入力してサインインしてください
              </p>
            </div>
            <p v-if="passwordChanged" class="rounded-md border bg-secondary p-3 text-sm">
              パスワードを変更しました。すべてのセッションとパーソナルアクセストークンが失効したため、再度サインインしてください。
            </p>
            <!--
              onBlur だと、一度エラーを出した後に入力を直しても次にフォーカスを外すまで
              エラーが残る（onBlur 由来のエラーが errorMap に残るため）。onChange で
              毎回検証し、表示は isBlurred で抑えて入力途中のエラー表示は避ける
              （isTouched は change でも true になるので使えない）。
              一度もフォーカスを外さずに送信した場合は理由が見えなくなるため、
              送信を試みた後は isBlurred に関係なく表示する
            -->
            <form.Field name="email" :validators="{ onChange: type('string.email') }">
              <template #default="{ field }">
                <Field>
                  <FieldLabel :for="field.name">メールアドレス</FieldLabel>
                  <Input
                    :id="field.name"
                    :name="field.name"
                    type="email"
                    placeholder="m@example.com"
                    autocomplete="email"
                    :model-value="field.state.value"
                    @blur="field.handleBlur"
                    @update:model-value="(v) => field.handleChange(String(v))"
                  />
                  <FieldError class="min-h-[1.25rem]">
                    {{
                      field.state.meta.errors.length &&
                      (field.state.meta.isBlurred || submitAttempted)
                        ? arkMessage(String(field.state.meta.errors[0]))
                        : ''
                    }}
                  </FieldError>
                </Field>
              </template>
            </form.Field>
            <form.Field name="password" :validators="{ onBlur: type('string >= 8') }">
              <template #default="{ field }">
                <Field>
                  <div class="flex items-center justify-between">
                    <FieldLabel :for="field.name">パスワード</FieldLabel>
                    <a
                      href="/auth/reset-password"
                      class="text-muted-foreground text-xs underline underline-offset-4"
                    >
                      パスワードをお忘れですか?
                    </a>
                  </div>
                  <PasswordInput
                    :id="field.name"
                    :name="field.name"
                    autocomplete="current-password"
                    :model-value="field.state.value"
                    @update:model-value="field.handleChange"
                    @blur="field.handleBlur"
                  />
                  <FieldError class="min-h-[1.25rem]">
                    {{
                      field.state.meta.errors.length
                        ? arkMessage(String(field.state.meta.errors[0]))
                        : ''
                    }}
                  </FieldError>
                </Field>
              </template>
            </form.Field>
            <p v-if="submitError" class="text-destructive text-center text-sm">
              {{ submitError }}
            </p>
            <!--
              canSubmit で無効化すると、入力途中の不正値でボタンだけが押せなくなり
              理由が画面に出ない。送信は submit 起因の検証に任せ、押せる状態を保つ
            -->
            <form.Subscribe>
              <template #default="{ isSubmitting }">
                <Field>
                  <Button type="submit" class="w-full" :disabled="isSubmitting || !isHydrated">
                    {{ isSubmitting ? 'サインイン中…' : 'サインイン' }}
                  </Button>
                </Field>
              </template>
            </form.Subscribe>
            <OAuthButtons redirect-after="/" error-redirect-after="/signin" />
            <FieldDescription class="text-center">
              アカウントをお持ちでない方は
              <a href="/signup" class="underline underline-offset-4">新規登録</a>
            </FieldDescription>
          </FieldGroup>
        </HydrationSafeForm>
        <div class="bg-muted relative hidden md:block">
          <img
            src="/placeholder.svg"
            alt=""
            class="absolute inset-0 h-full w-full object-cover dark:brightness-[0.2] dark:grayscale"
          />
        </div>
      </CardContent>
    </Card>
  </div>
</template>
