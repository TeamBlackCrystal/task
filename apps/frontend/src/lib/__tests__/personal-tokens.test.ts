import { describe, it, expect } from 'vitest';
import {
  EXPIRATION_PRESETS,
  SCOPE_CATALOG,
  expiresAtFromPreset,
  formatExpiry,
  formatLastUsed,
  maskedToken,
} from '../personal-tokens';

const NOW = new Date('2026-08-24T12:00:00Z');

describe('expiresAtFromPreset', () => {
  it.each([
    ['30d', 30],
    ['90d', 90],
    ['1y', 365],
  ] as const)('%s は %i 日後の ISO 日時を返す', (key, days) => {
    const iso = expiresAtFromPreset(key, NOW);
    expect(iso).toBe(new Date(NOW.getTime() + days * 86400000).toISOString());
  });

  it('無期限は null を返す', () => {
    expect(expiresAtFromPreset('none', NOW)).toBeNull();
  });

  it('プリセットは 4 種類（UI のボタン数と一致）', () => {
    expect(EXPIRATION_PRESETS).toHaveLength(4);
  });
});

describe('maskedToken', () => {
  it('末尾 4 文字だけを見せる', () => {
    expect(maskedToken('7f3a')).toBe('pat_••••••7f3a');
  });
});

describe('formatExpiry', () => {
  it('null は無期限', () => {
    expect(formatExpiry(null, NOW)).toBe('無期限');
    expect(formatExpiry(undefined, NOW)).toBe('無期限');
  });

  it('未来の日付は「まで有効」', () => {
    expect(formatExpiry('2026-11-14T00:00:00Z', NOW)).toContain('まで有効');
  });

  it('過去の日付は「に期限切れ」', () => {
    expect(formatExpiry('2026-01-01T00:00:00Z', NOW)).toContain('に期限切れ');
  });
});

describe('formatLastUsed', () => {
  it('null は未使用', () => {
    expect(formatLastUsed(null, NOW)).toBe('未使用');
  });

  it('1 分未満はたった今', () => {
    expect(formatLastUsed('2026-08-24T11:59:30Z', NOW)).toBe('たった今使用');
  });

  it('分・時間・日の相対表示', () => {
    expect(formatLastUsed('2026-08-24T11:15:00Z', NOW)).toBe('45分前に使用');
    expect(formatLastUsed('2026-08-24T09:00:00Z', NOW)).toBe('3時間前に使用');
    expect(formatLastUsed('2026-08-22T12:00:00Z', NOW)).toBe('2日前に使用');
  });

  it('境界: ちょうど 60 分は「1時間前」、24 時間は「1日前」', () => {
    expect(formatLastUsed('2026-08-24T11:00:00Z', NOW)).toBe('1時間前に使用');
    expect(formatLastUsed('2026-08-23T12:00:00Z', NOW)).toBe('1日前に使用');
  });

  it('30 日以上前は絶対日付', () => {
    expect(formatLastUsed('2026-06-01T00:00:00Z', NOW)).toMatch(/2026.*に使用/);
  });
});

describe('SCOPE_CATALOG', () => {
  it('重複がなく、backend の全 11 スコープを網羅する', () => {
    const scopes = SCOPE_CATALOG.map((entry) => entry.scope);
    expect(new Set(scopes).size).toBe(scopes.length);
    expect(scopes).toHaveLength(11);
  });
});
