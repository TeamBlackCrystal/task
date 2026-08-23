import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createRenderer } from '../markup-renderer';
import { createRehypeStarryNight, starryNightSanitizeSchema } from '../rehype-starry-night';
import { gfmSanitizeSchema, remarkGfm } from '../remark-gfm';

/**
 * 初期化共有 (seam の renderer スコープ共有) の失敗経路。
 * 共有は「初期化に一度失敗すると全 render が永久に落ちる」単一障害点になり得るため、
 * 失敗した共有実体を捨てて次の描画で作り直すことを機械で固定する。
 * upstream は初期化失敗の promise を transformer 内に抱え続ける (poisoned promise) —
 * 1 回目の factory だけ常時 reject する transformer を返す mock でそれを再現する。
 */
const upstreamFactory = vi.hoisted(() => ({ calls: 0 }));

vi.mock('rehype-starry-night', async (importOriginal) => {
  const actual = await importOriginal<typeof import('rehype-starry-night')>();
  return {
    default: function rehypeStarryNightFailFirst(...args: Parameters<typeof actual.default>) {
      upstreamFactory.calls += 1;
      if (upstreamFactory.calls === 1) {
        // poisoned promise の再現: この transformer は何度呼ばれても reject し続ける。
        // カウンタは beforeEach でリセットされるため「各試験の 1 個目の実体が毒」になる
        return async () => {
          throw new Error('starry-night init failure (injected)');
        };
      }
      return actual.default(...args);
    },
  };
});

function createHighlightRenderer() {
  return createRenderer({
    profiles: {
      github: { remarkPlugins: [remarkGfm], rehypePlugins: [createRehypeStarryNight()] },
    },
    sanitizeSchemas: [gfmSanitizeSchema, starryNightSanitizeSchema],
  });
}

beforeEach(() => {
  upstreamFactory.calls = 0;
});

describe('starry-night 初期化失敗 (共有実体の捨てて再試行)', () => {
  it('初期化失敗は共有実体を捨て、次の描画で作り直して復旧する', async () => {
    const render = createHighlightRenderer();
    const fixture = '```ts\nconst a = 1;\n```';
    await expect(render(fixture, { scope: 'comment-1' })).rejects.toThrow('injected');
    // 同一入力の再描画で復旧する (失敗時は HTML キャッシュに何も残っていない)
    const html = await render(fixture, { scope: 'comment-1' });
    expect(html).toContain('class="pl-');
    // 作り直しは失敗時のみ = factory はちょうど 2 回 (成功後にまた増えない)
    const again = await render('```ts\nconst b = 2;\n```', { scope: 'comment-2' });
    expect(again).toContain('class="pl-');
    expect(upstreamFactory.calls).toBe(2);
  });

  it('既定 prefix (memoize 済み processor) 経由でも失敗から復旧する', async () => {
    // 共有実体の解決は attach 時ではなく transform 時 — memoize 済み processor が
    // poisoned 実体を掴んだまま残らないことの固定。
    const render = createHighlightRenderer();
    const fixture = '```ts\nconst c = 3;\n```';
    await expect(render(fixture)).rejects.toThrow('injected');
    const html = await render(fixture);
    expect(html).toContain('class="pl-');
  });
});
