import { describe, expect, it } from 'vitest';

import { isKnownProvider, providerLabel } from '@/lib/oauth-providers';

describe('oauth-providers', () => {
  it('知っているプロバイダーは表示名を返す', () => {
    expect(providerLabel('gitlab_selfhosted')).toBe('GitLab (セルフホスト)');
    expect(isKnownProvider('gitlab_selfhosted')).toBe(true);
  });

  it('知らないプロバイダーはそのままの文字列を返す', () => {
    expect(providerLabel('unknown_provider')).toBe('unknown_provider');
    expect(isKnownProvider('unknown_provider')).toBe(false);
  });

  it('Object.prototype のキーを prototype 側の値として拾わない', () => {
    for (const key of ['constructor', 'toString', 'hasOwnProperty', '__proto__']) {
      expect(providerLabel(key)).toBe(key);
      expect(isKnownProvider(key)).toBe(false);
    }
  });
});
