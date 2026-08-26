import { describe, expect, it } from 'vitest';
import { avatarInitials } from '@/lib/initials';

describe('avatarInitials', () => {
  it('先頭 2 文字を大文字で返す', () => {
    expect(avatarInitials('yupix')).toBe('YU');
  });

  it('2 文字目がサロゲートペアでも分割しない', () => {
    // slice(0, 2) だと「a」+ 絵文字の前半サロゲートになり壊れる。
    expect(avatarInitials('a😀b')).toBe('A😀');
  });

  it('先頭がサロゲートペアでも分割しない', () => {
    expect(avatarInitials('😀😀😀')).toBe('😀😀');
  });

  it('count=1 でも壊れた半分を返さない', () => {
    expect(avatarInitials('😀x', 1)).toBe('😀');
  });

  it('空文字は空文字を返す', () => {
    expect(avatarInitials('')).toBe('');
  });
});
