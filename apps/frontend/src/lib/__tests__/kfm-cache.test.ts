import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { LRUCache } from 'lru-cache';
import type { PluggableList } from 'unified';
import { describe, expect, expectTypeOf, it, vi } from 'vitest';
import { buildCacheKey } from '../markup-renderer/_cache';
import { createRenderer } from '../markup-renderer/_renderer';
import { createRehypeStarryNight } from '../rehype-starry-night';
import { gfmSanitizeSchema, remarkGfm } from '../remark-gfm';
import { koyoriAlertsSanitizeSchema, remarkKoyoriAlerts } from '../remark-koyori-alerts';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

function createGithubRenderer(cache: LRUCache<string, string>) {
  return createRenderer({
    profiles: { github: { remarkPlugins: [remarkGfm, remarkKoyoriAlerts] } },
    sanitizeSchemas: [gfmSanitizeSchema, koyoriAlertsSanitizeSchema],
    cache,
  });
}

/** L1 キーで禁止している djb2 (32bit)。衝突ペア探索と回帰ガードのためテスト内に複製 */
function djb2(value: string): number {
  let hash = 5381;
  for (let index = 0; index < value.length; index++) {
    hash = (hash * 33 + value.charCodeAt(index)) >>> 0;
  }
  return hash;
}

// 事前計算した djb2 衝突ペア (seed 固定 LCG で 74,619 件目に発見・誕生日境界どおり)。
// 下の陽性対照が実際に衝突していることを毎回検証するため、値の捏造はテストを通らない。
const DJB2_COLLISION_PAIR = [
  'kfm-collision-probe-bjo0mx-ltebt0',
  'kfm-collision-probe-1xqhhu3-8f19pq',
] as const;

describe('L1 キャッシュ (🔴 full-text キー・ハッシュ禁止)', () => {
  it('djb2 衝突ペアでも別本文は別 HTML を返す (キーを hash に戻すと落ちる)', async () => {
    const [first, second] = DJB2_COLLISION_PAIR;
    // 陽性対照: ペアは実際に djb2 衝突している (この試験が衝突を見ていることの証明)
    expect(first).not.toBe(second);
    expect(djb2(first)).toBe(djb2(second));

    const cache = new LRUCache<string, string>({ max: 100 });
    const render = createGithubRenderer(cache);
    const htmlFirst = await render(first);
    const htmlSecond = await render(second);
    expect(htmlFirst).toContain(first);
    // djb2 キーだとここで htmlFirst (別本文の HTML) が返り落ちる
    expect(htmlSecond).toContain(second);
    expect(htmlSecond).not.toContain(first);
  });

  it('キャッシュキーは本文 full-text をそのまま含む (形式ガード)', async () => {
    const cache = new LRUCache<string, string>({ max: 100 });
    const render = createGithubRenderer(cache);
    const text = '# full-text キー確認\n\n本文そのもの';
    await render(text);
    const keys = [...cache.keys()];
    expect(keys).toHaveLength(1);
    expect(keys[0]!.endsWith(text)).toBe(true);
  });

  it('同一入力 2 回目はキャッシュから返る (キャッシュ経路の陽性対照)', async () => {
    const cache = new LRUCache<string, string>({ max: 100 });
    const render = createGithubRenderer(cache);
    const text = 'キャッシュ経路の確認';
    await render(text);
    expect(cache.size).toBe(1);
    // キャッシュ値を直接改ざんし、2 回目がキャッシュを経由することを決定的に観測する
    const key = [...cache.keys()][0]!;
    cache.set(key, 'CACHED_SENTINEL');
    expect(await render(text)).toBe('CACHED_SENTINEL');
  });
});

describe('buildCacheKey (fingerprint / profile / scope / config 分離)', () => {
  it('fingerprint・profile・scope・config・本文のどれが違ってもキーが変わる', () => {
    const base = buildCacheKey('fp1', 'github', '', '{"a":1}', '本文');
    expect(buildCacheKey('fp2', 'github', '', '{"a":1}', '本文')).not.toBe(base);
    expect(buildCacheKey('fp1', 'kfm', '', '{"a":1}', '本文')).not.toBe(base);
    // scope 違いを同一視すると別 scope の脚注 id 付き HTML を返してしまう
    expect(buildCacheKey('fp1', 'github', 'task-1', '{"a":1}', '本文')).not.toBe(base);
    expect(buildCacheKey('fp1', 'github', '', '{"a":2}', '本文')).not.toBe(base);
    expect(buildCacheKey('fp1', 'github', '', '{"a":1}', '別本文')).not.toBe(base);
  });

  it('前置部と本文の境界が曖昧にならない (区切りを含む値でも衝突しない)', () => {
    // 前置部の値に区切りらしき文字列が入っても、JSON 正規化 + NUL 区切りで一意
    const tricky = buildCacheKey('fp"] x', 'github', '', '{}', 'text');
    const straight = buildCacheKey('fp', 'github', '', '{}', '"] x\u0000text');
    expect(tricky).not.toBe(straight);
  });
});

describe('pipeline fingerprint (設定変更で旧エントリを拾わない)', () => {
  it('starry-night factory は fingerprint が観測できない options 口を持たない', () => {
    // Function.length だけでは `(options = {})` が 0 になり素通りするため、型の引数 tuple
    // も空であることを固定する。引数を足すなら fingerprint 直列化とキー分離試験を
    // 同じ変更で追加すること。
    expect(createRehypeStarryNight.length).toBe(0);
    expectTypeOf(createRehypeStarryNight).parameters.toEqualTypeOf<[]>();
  });

  it('composition root は重量級 starry-night seam を静的 import しない', () => {
    const source = fs.readFileSync(path.join(__dirname, '../markup-renderer/index.ts'), 'utf8');
    // named/default import だけでなく、副作用 import と static re-export も重量級 seam を
    // 静的グラフへ戻す。同じ specifier の静的構文を三形とも塞ぐ。
    expect(source).not.toMatch(
      /^\s*import\s+(?!type\b)(?:(?:[^;]*\sfrom\s+)?['"]@\/lib\/rehype-starry-night['"])/m,
    );
    expect(source).not.toMatch(
      /^\s*export\s+(?:\*|\{[^}]*\})\s+from\s+['"]@\/lib\/rehype-starry-night['"]/m,
    );
    // 型位置の `typeof import(...)` だけ残って実行時 import が消えても通らないよう、
    // type query を除去したソースで dynamic import の実在を固定する。
    const sourceWithoutTypeQueries = source.replace(/typeof\s+import\([^)]*\)/g, '');
    expect(sourceWithoutTypeQueries).toContain("import('@/lib/rehype-starry-night')");
  });

  it('starry-night の import/factory Promise が reject したら次の描画で再試行する', async () => {
    vi.resetModules();
    let moduleLoads = 0;
    vi.doMock('@/lib/rehype-starry-night', () => {
      moduleLoads += 1;
      if (moduleLoads === 1) throw new Error('poisoned dynamic import (injected)');
      return { createRehypeStarryNight: () => () => async (tree: unknown) => tree };
    });

    const { renderDescription: isolatedRenderDescription } = await import('../markup-renderer');
    // Vitest は mock module factory の例外を import 失敗としてラップするため、ここでは
    // reject 自体と、次行の再 import 成功・moduleLoads === 2 を機構として確認する。
    await expect(isolatedRenderDescription('first import attempt')).rejects.toThrow();
    await expect(isolatedRenderDescription('second import attempt')).resolves.toContain(
      'second import attempt',
    );
    expect(moduleLoads).toBe(2);

    vi.doUnmock('@/lib/rehype-starry-night');
  });

  it('sanitize スキーマが違う renderer は同一本文でも別キャッシュエントリになる', async () => {
    const shared = new LRUCache<string, string>({ max: 100 });
    const renderA = createRenderer({
      profiles: { github: { remarkPlugins: [remarkGfm, remarkKoyoriAlerts] } },
      sanitizeSchemas: [gfmSanitizeSchema, koyoriAlertsSanitizeSchema],
      cache: shared,
    });
    const renderB = createRenderer({
      profiles: { github: { remarkPlugins: [remarkGfm, remarkKoyoriAlerts] } },
      sanitizeSchemas: [gfmSanitizeSchema],
      cache: shared,
    });
    await renderA('同一本文');
    await renderB('同一本文');
    expect(shared.size).toBe(2);
  });

  it('content-scope config が違う renderer も別キャッシュエントリになる', async () => {
    const shared = new LRUCache<string, string>({ max: 100 });
    const make = (config: unknown) =>
      createRenderer({
        profiles: { github: { remarkPlugins: [remarkGfm, remarkKoyoriAlerts] } },
        sanitizeSchemas: [gfmSanitizeSchema, koyoriAlertsSanitizeSchema],
        contentConfig: config,
        cache: shared,
      });
    await make({ defaultProfile: 'github' })('同一本文');
    await make({ defaultProfile: 'github', maxDecorations: 8 })('同一本文');
    expect(shared.size).toBe(2);
  });

  it('rehypePlugins だけが違う renderer は同一本文でも別キャッシュエントリになる', async () => {
    // fingerprint が rehype 層を見ていないと、着色ありの renderer が着色なしの旧 HTML を
    // (またはその逆を) キャッシュから返す。ここはその回帰ガード。
    const shared = new LRUCache<string, string>({ max: 100 });
    const make = (rehypePlugins?: PluggableList) =>
      createRenderer({
        profiles: {
          github: {
            remarkPlugins: [remarkGfm, remarkKoyoriAlerts],
            ...(rehypePlugins === undefined ? {} : { rehypePlugins }),
          },
        },
        sanitizeSchemas: [gfmSanitizeSchema, koyoriAlertsSanitizeSchema],
        cache: shared,
      });
    // コードフェンスを含まない入力 = 両 renderer の出力 HTML は同一。それでもエントリが
    // 分かれることが「キー差は fingerprint 由来であって出力差ではない」ことの証明になる。
    const input = 'rehype 層 fingerprint の確認 (フェンスなし)';
    const withoutRehype = await make()(input);
    const withRehype = await make([createRehypeStarryNight()])(input);
    expect(withoutRehype).toBe(withRehype);
    expect(shared.size).toBe(2);
  });

  it('rehypePlugins が同一構成なら同一エントリを共有する (上の試験の陽性対照)', async () => {
    // 独立 renderer 同士でも構成が同じならキーが一致する = 上の試験の size 2 が
    // 「インスタンス差」でなく「rehype 層の構成差」から来ていることを固定する。
    const shared = new LRUCache<string, string>({ max: 100 });
    const make = () =>
      createRenderer({
        profiles: {
          github: {
            remarkPlugins: [remarkGfm, remarkKoyoriAlerts],
            // renderer ごとに factory を呼んでも attacher 名は同じ = fingerprint が一致する
            rehypePlugins: [createRehypeStarryNight()],
          },
        },
        sanitizeSchemas: [gfmSanitizeSchema, koyoriAlertsSanitizeSchema],
        cache: shared,
      });
    const input = 'rehype 層 fingerprint の確認 (フェンスなし)';
    await make()(input);
    await make()(input);
    expect(shared.size).toBe(1);
  });
});
