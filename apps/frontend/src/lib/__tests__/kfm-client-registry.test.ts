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

  it('既定の登録タグは kfm-mermaid の 1 件 (Phase 2 第一号)', () => {
    const result = registerKfmCustomElements();
    expect(result).toEqual({ skipped: false, defined: 1 });
    expect(customElements.get('kfm-mermaid')).toBeTypeOf('function');
  });
});

describe('composition root の client 汚染ガード (🔴 バンドル退行の再発機構)', () => {
  it('root は client registry を再エクスポートしない (再エクスポートを戻すと落ちる)', async () => {
    // root は import しただけで createRenderer が副作用で走る。root から
    // registerKfmCustomElements を再エクスポートすると、client entry が root 経由で
    // import する退行 (+205 kB 実測) の再発経路になるため、直接経路
    // (@/lib/markup-renderer/_client-registry) だけを残す。NOTE コメントは機構では
    // ないので、この試験が機構 (再追加した瞬間に赤くなる関門) である。
    const root = await import('../markup-renderer');
    expect(Object.keys(root)).not.toContain('registerKfmCustomElements');
  });
});
