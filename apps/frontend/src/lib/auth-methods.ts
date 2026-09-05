import type { components } from '@/generated/api';

export type OAuthConnection = components['schemas']['OAuthConnectionItem'];
export type OAuthProvider = components['schemas']['OAuthProviderItem'];

/** backend の `SetPasswordRequest` / `PasswordChangeBody` と同じ下限。 */
export const PASSWORD_MIN_LENGTH = 8;

/**
 * 使える認証方法の数。
 *
 * backend は「その OAuth 連携が最後の1つで、パスワードもパスキーも無い」ときだけ解除を拒む。
 * 画面でも同じ数え方をして事前に注意を出すが、最終判断はサーバーの LastAuthMethod に任せる。
 * 画面が数えた後に別のタブで増減されることがあるため、ここでの判定は先回りの案内に留める。
 */
export function countAuthMethods(input: {
  hasPassword: boolean;
  connectionCount: number;
  passkeyCount: number;
}): number {
  return (input.hasPassword ? 1 : 0) + input.connectionCount + input.passkeyCount;
}

/** 接続日時。時刻まで出すと行が詰まるので日付だけにする。 */
export function formatConnectedAt(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return '';
  return date.toLocaleDateString('ja-JP', { year: 'numeric', month: 'numeric', day: 'numeric' });
}

/** 初回設定か変更か。入力欄も送信先も変わる。 */
export type PasswordFormMode = 'set' | 'change';

export type PasswordFormValues = {
  current: string;
  next: string;
  confirm: string;
};

export type PasswordFormErrors = {
  current?: string;
  next?: string;
  confirm?: string;
};

/**
 * パスワード欄の検証。
 *
 * 初回設定には現在のパスワードが無いので、その欄ごと出さない。変更のときだけ
 * 「現在のパスワードと同じ」を弾く（同じ値で送ると変更した気になって失効だけ起きる）。
 */
export function validatePasswordForm(
  mode: PasswordFormMode,
  values: PasswordFormValues,
): PasswordFormErrors {
  const errors: PasswordFormErrors = {};

  if (mode === 'change' && values.current.length === 0) {
    errors.current = '現在のパスワードを入力してください。';
  }

  if (values.next.length < PASSWORD_MIN_LENGTH) {
    errors.next = `パスワードは${PASSWORD_MIN_LENGTH}文字以上で入力してください。`;
  } else if (mode === 'change' && values.next === values.current) {
    errors.next = '現在のパスワードとは異なるものにしてください。';
  }

  if (values.confirm !== values.next) {
    errors.confirm = 'パスワードが一致しません。';
  }

  return errors;
}

export function hasPasswordFormError(errors: PasswordFormErrors): boolean {
  return Object.values(errors).some((message) => Boolean(message));
}
