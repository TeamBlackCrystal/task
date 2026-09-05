import { describe, it, expect } from 'vitest';
import {
  countAuthMethods,
  formatConnectedAt,
  hasPasswordFormError,
  validatePasswordForm,
} from '@/lib/auth-methods';

describe('countAuthMethods', () => {
  it('パスワード・連携・パスキーを合わせて数える', () => {
    expect(countAuthMethods({ hasPassword: true, connectionCount: 2, passkeyCount: 1 })).toBe(4);
  });

  it('パスワードが無ければ数に入れない', () => {
    expect(countAuthMethods({ hasPassword: false, connectionCount: 1, passkeyCount: 0 })).toBe(1);
  });

  /** backend は passkey も数えて最後の1つを守る。ここが食い違うと誤った注意が出る。 */
  it('パスキーだけでも認証方法として数える', () => {
    expect(countAuthMethods({ hasPassword: false, connectionCount: 1, passkeyCount: 2 })).toBe(3);
  });
});

describe('validatePasswordForm', () => {
  it('初回設定では現在のパスワードを求めない', () => {
    const errors = validatePasswordForm('set', {
      current: '',
      next: 'NewPassword123',
      confirm: 'NewPassword123',
    });
    expect(errors).toEqual({});
    expect(hasPasswordFormError(errors)).toBe(false);
  });

  it('変更では現在のパスワードが要る', () => {
    const errors = validatePasswordForm('change', {
      current: '',
      next: 'NewPassword123',
      confirm: 'NewPassword123',
    });
    expect(errors.current).toBe('現在のパスワードを入力してください。');
    expect(hasPasswordFormError(errors)).toBe(true);
  });

  it('8文字未満を弾く', () => {
    const errors = validatePasswordForm('set', { current: '', next: 'short7', confirm: 'short7' });
    expect(errors.next).toBe('パスワードは8文字以上で入力してください。');
  });

  /** 境界のちょうど 8 文字は通す。 */
  it('ちょうど8文字は通す', () => {
    const errors = validatePasswordForm('set', {
      current: '',
      next: '12345678',
      confirm: '12345678',
    });
    expect(errors.next).toBeUndefined();
  });

  it('現在のパスワードと同じ値への変更を弾く', () => {
    const errors = validatePasswordForm('change', {
      current: 'SamePassword123',
      next: 'SamePassword123',
      confirm: 'SamePassword123',
    });
    expect(errors.next).toBe('現在のパスワードとは異なるものにしてください。');
  });

  it('確認が一致しないと弾く', () => {
    const errors = validatePasswordForm('set', {
      current: '',
      next: 'NewPassword123',
      confirm: 'NewPassword124',
    });
    expect(errors.confirm).toBe('パスワードが一致しません。');
  });
});

describe('formatConnectedAt', () => {
  it('日付として表示する', () => {
    expect(formatConnectedAt('2026-09-05T07:22:52Z')).toMatch(/2026/);
  });

  it('壊れた値では空文字にする', () => {
    expect(formatConnectedAt('not-a-date')).toBe('');
  });
});
