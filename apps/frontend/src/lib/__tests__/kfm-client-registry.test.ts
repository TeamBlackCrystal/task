import { afterEach, describe, expect, it, vi } from 'vitest';
import { registerKfmCustomElements } from '../markup-renderer/_client-registry';
import type { KfmCustomElementDefinition } from '../markup-renderer/_client-registry';

const probeDefinition: KfmCustomElementDefinition = [
  'kfm-test-probe',
  () => class extends HTMLElement {},
];

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('registerKfmCustomElements (🔴 client ガード)', () => {
  it('customElements 不在 (SSR / Node) では throw せず skip する (ガードを外すと落ちる)', () => {
    vi.stubGlobal('customElements', undefined);
    let result: ReturnType<typeof registerKfmCustomElements> | undefined;
    expect(() => {
      result = registerKfmCustomElements([probeDefinition]);
    }).not.toThrow();
    expect(result).toEqual({ skipped: true, defined: 0 });
  });

  it('ブラウザ環境では定義を実際に登録する (ガード試験の陽性対照)', () => {
    const result = registerKfmCustomElements([probeDefinition]);
    expect(result).toEqual({ skipped: false, defined: 1 });
    expect(customElements.get('kfm-test-probe')).toBeTypeOf('function');
  });

  it('再呼び出しで二重 define 例外を出さない (HMR 安全)', () => {
    registerKfmCustomElements([probeDefinition]);
    const second = registerKfmCustomElements([probeDefinition]);
    expect(second).toEqual({ skipped: false, defined: 0 });
  });

  it('既定 (Phase 1) の登録タグは空 = seam のみ', () => {
    const result = registerKfmCustomElements();
    expect(result).toEqual({ skipped: false, defined: 0 });
  });
});
