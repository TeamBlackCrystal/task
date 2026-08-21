import { describe, expect, it } from 'vitest';
import type { KfmContentConfig, KfmProfile } from '../markup-renderer';
import { resolveContentConfig } from '../markup-renderer';

describe('resolveContentConfig (層のキー単位スパース上書き)', () => {
  it('層なしはコード既定 (github)', () => {
    expect(resolveContentConfig()).toEqual({ defaultProfile: 'github' });
    expect(resolveContentConfig({})).toEqual({ defaultProfile: 'github' });
  });

  it('undefined のキーは下層へ fall through する', () => {
    expect(resolveContentConfig({ defaultProfile: undefined })).toEqual({
      defaultProfile: 'github',
    });
  });

  it('universe 内の値は上書きされる陽性対照', () => {
    // Phase 1 の universe は github のみのため、既定と同値の上書きで機構を固定する
    expect(resolveContentConfig({ defaultProfile: 'github' })).toEqual({
      defaultProfile: 'github',
    });
  });

  it('universe 外の defaultProfile は fail-fast で throw (層は値を持ち込めない)', () => {
    expect(() => resolveContentConfig({ defaultProfile: 'evil' as KfmProfile })).toThrow(
      'safe universe',
    );
  });

  it('未知キーはコピーされない (層内容の無検証コピー禁止)', () => {
    const layer = {
      defaultProfile: 'github',
      __proto__constructor: 'x',
      injected: '<script>',
    } as Partial<KfmContentConfig>;
    const resolved = resolveContentConfig(layer);
    expect(resolved).toEqual({ defaultProfile: 'github' });
    expect(Object.keys(resolved)).toEqual(['defaultProfile']);
  });
});
