import { describe, expect, it } from 'vitest';
import { createRenderer } from '../markup-renderer';
import { gfmSanitizeSchema, remarkGfm } from '../remark-gfm';

/**
 * processor 構築 (buildProcessor) の回数を rehype 層の attacher 呼び出しで機械計数する。
 * unified は freeze() でプラグイン attacher を 1 回呼ぶため、attacher 呼び出し回数 =
 * processor 構築回数。starry-night の文法初期化回数は kfm-starry-night-init-count.test.ts
 * が別途数える (構築回数と初期化回数は seam の renderer スコープ共有により独立)。
 */
function makeBuildCounter() {
  const counter = { builds: 0 };
  function buildCountingRehypePlugin() {
    counter.builds += 1;
  }
  return { counter, plugin: buildCountingRehypePlugin };
}

function createCountingRenderer(plugin: () => void) {
  return createRenderer({
    profiles: { github: { remarkPlugins: [remarkGfm], rehypePlugins: [plugin] } },
    sanitizeSchemas: [gfmSanitizeSchema],
  });
}

describe('processor memoize (既定 prefix)', () => {
  it('scope 無し描画は本文が違っても processor を 1 回だけ構築する (従来 memoize の回帰)', async () => {
    const { counter, plugin } = makeBuildCounter();
    const render = createCountingRenderer(plugin);
    await render('一つ目');
    await render('二つ目');
    expect(counter.builds).toBe(1);
  });
});

describe('processor 構築 (scope 付き prefix)', () => {
  it('scope 付きは都度構築する (意図した設計: scope の値空間は非有界で溜めない)', async () => {
    // 都度構築が許されるのは、高い初期化が seam 側で共有されているから
    // (kfm-starry-night-init-count.test.ts が初期化 1 回を固定)。この試験は
    // 「scope 付き processor を溜め込まない」設計判断そのものを固定する。
    const { counter, plugin } = makeBuildCounter();
    const render = createCountingRenderer(plugin);
    const scopes = ['comment-1', 'comment-2', 'comment-3'];
    for (const scope of scopes) {
      await render(`${scope} の本文`, { scope });
    }
    expect(counter.builds).toBe(scopes.length);
    // 同一入力・同一 scope の再描画は L1 (HTML) キャッシュが吸収し、構築へ到達しない
    await render('comment-1 の本文', { scope: 'comment-1' });
    expect(counter.builds).toBe(scopes.length);
  });
});
