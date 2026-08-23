import { beforeEach, describe, expect, it, vi } from 'vitest';
import upstreamRehypeStarryNight from 'rehype-starry-night';
import { createRenderer, renderDescription } from '../markup-renderer';
import { createRehypeStarryNight, starryNightSanitizeSchema } from '../rehype-starry-night';
import { gfmSanitizeSchema, remarkGfm } from '../remark-gfm';

/**
 * starry-night 初期化「回数」の機械計数。
 * upstream factory (rehype-starry-night の default export) は呼ばれた時点で
 * createStarryNight (onig.wasm ＋ common 文法一式の登録) を同期的に開始するため
 * (rehype-starry-night@2.2.0 lib/index.js 実物確認)、factory 呼び出し回数 =
 * 文法初期化回数。時間や体感ではなく回数そのものを数える。
 * production composition root と seam (../rehype-starry-night) は共に本 mock 経由で
 * upstream を掴むため、本番配線・独自 renderer・旧配線を同じ計器で数えられる。
 */
const upstreamFactory = vi.hoisted(() => ({ calls: 0 }));

vi.mock('rehype-starry-night', async (importOriginal) => {
  const actual = await importOriginal<typeof import('rehype-starry-night')>();
  return {
    default: function rehypeStarryNightCounted(...args: Parameters<typeof actual.default>) {
      upstreamFactory.calls += 1;
      return actual.default(...args);
    },
  };
});

type RehypePlugin = () => ReturnType<typeof upstreamRehypeStarryNight>;

function createHighlightRenderer(plugin: RehypePlugin) {
  return createRenderer({
    profiles: {
      github: { remarkPlugins: [remarkGfm], rehypePlugins: [plugin] },
    },
    sanitizeSchemas: [gfmSanitizeSchema, starryNightSanitizeSchema],
  });
}

// 本文は scope ごとに変えて L1 (HTML) キャッシュを必ず外す — HTML キャッシュヒットで
// processor 経路自体を通らない描画を数えても、初期化回数の主張にならない。
function fixtureFor(name: string): string {
  return `\`\`\`ts\nconst ${name.replace(/-/g, '_')} = 1;\n\`\`\``;
}

beforeEach(() => {
  upstreamFactory.calls = 0;
});

describe('starry-night 初期化回数 (production composition root)', () => {
  it('本番 renderDescription は異なる scope の N 回描画でも初期化が 1 回', async () => {
    const scopes = ['production-1', 'production-2', 'production-3'];
    for (const scope of scopes) {
      expect(await renderDescription(fixtureFor(scope), { scope })).toContain('class="pl-');
    }
    expect(upstreamFactory.calls).toBe(1);
  });
});

describe('starry-night 初期化回数 (factory closure を独自 renderer へ注入)', () => {
  it('異なる scope で N 回描画しても文法初期化は 1 回だけ走る', async () => {
    const render = createHighlightRenderer(createRehypeStarryNight());
    const scopes = ['comment-1', 'comment-2', 'comment-3', 'comment-4', 'comment-5'];
    for (const scope of scopes) {
      const html = await render(fixtureFor(scope), { scope });
      // 計器の陽性対照 (経路確認): 着色が実際に効いた出力を確認する。これが無いと
      // 「初期化 0 回で何も描画されなかった」場合も calls === 1 を満たしてしまい、
      // 計数が着色経路を通っている保証にならない。
      expect(html).toContain('class="pl-');
    }
    expect(upstreamFactory.calls).toBe(1);
  });

  it('既定 prefix (scope 無し) の描画を混ぜても初期化は増えない', async () => {
    const render = createHighlightRenderer(createRehypeStarryNight());
    const scoped = await render(fixtureFor('scoped'), { scope: 'comment-1' });
    const plain = await render(fixtureFor('no-scope'));
    expect(scoped).toContain('class="pl-');
    expect(plain).toContain('class="pl-');
    expect(upstreamFactory.calls).toBe(1);
  });

  it('factory を呼び直した独自 renderer 同士は実体を共有しない', async () => {
    // seam の共有単位は factory closure。production singleton の module-level lazy Promise
    // とは別契約であり、独自 renderer は factory を呼び直せば隔離できる。
    const first = createHighlightRenderer(createRehypeStarryNight());
    const second = createHighlightRenderer(createRehypeStarryNight());
    expect(await first(fixtureFor('first'), { scope: 'a' })).toContain('class="pl-');
    expect(await second(fixtureFor('second'), { scope: 'b' })).toContain('class="pl-');
    expect(upstreamFactory.calls).toBe(2);
  });

  it('陽性対照: 旧配線 (upstream factory を rehypePlugins へ直挿し) は scope ごとに初期化が走る', async () => {
    // #584 レビュー指摘の再現 = この計器が再初期化を検出できることの実測。
    // 共有ラッパ導入前の composition root と同じ配線であり、修正前のコードに
    // この試験群を当てると上の「1 回だけ」が本試験と同じ N 回で赤くなる。
    const render = createHighlightRenderer(upstreamRehypeStarryNight);
    const scopes = ['comment-1', 'comment-2', 'comment-3'];
    for (const scope of scopes) {
      const html = await render(fixtureFor(scope), { scope });
      expect(html).toContain('class="pl-');
    }
    expect(upstreamFactory.calls).toBe(scopes.length);
  });
});
