import { describe, expect, it } from 'vitest';

import {
  connectionProviderLabel,
  connectionProviderSlug,
  isKnownProvider,
  providerLabel,
} from '@/lib/oauth-providers';

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

  it('OIDC の連携識別子は開始用 slug と表示名に変換する', () => {
    expect(connectionProviderSlug('oidc:https://idp.example.com')).toBe('oidc');
    expect(connectionProviderLabel('oidc:https://idp.example.com')).toBe('OIDC');
  });

  it('OIDC 以外の連携識別子は変えない', () => {
    expect(connectionProviderSlug('github')).toBe('github');
    expect(connectionProviderSlug('oidcish:https://idp.example.com')).toBe(
      'oidcish:https://idp.example.com',
    );
  });
});
