import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { defineComponent } from 'vue';
import { mount, flushPromises } from '@vue/test-utils';
import { VueQueryPlugin, QueryClient } from '@tanstack/vue-query';

import { useRenderedDescription, RENDER_DESCRIPTION_PATH } from '../useRenderedDescription';

const TASK_UUID = '11111111-2222-3333-4444-555555555555';
const DESCRIPTION = '# 見出し';

/** 1 回分の応答。順に取り出して返す。 */
type Reply = { status: number; html?: string | null };

describe('useRenderedDescription', () => {
  let queryClient: QueryClient;
  let replies: Reply[];
  let calls: number;
  let rendered: ReturnType<typeof useRenderedDescription>;

  function mountHost(description: string | null = DESCRIPTION) {
    const Host = defineComponent({
      setup() {
        rendered = useRenderedDescription(TASK_UUID, description);
        return () => null;
      },
    });
    return mount(Host, { global: { plugins: [[VueQueryPlugin, { queryClient }]] } });
  }

  beforeEach(() => {
    vi.useFakeTimers();
    queryClient = new QueryClient();
    replies = [];
    calls = 0;
    vi.stubGlobal('fetch', async (input: string) => {
      expect(input).toBe(RENDER_DESCRIPTION_PATH);
      const reply = replies[Math.min(calls, replies.length - 1)];
      calls += 1;
      return new Response(reply.status === 200 ? JSON.stringify({ html: reply.html }) : 'ng', {
        status: reply.status,
        headers: { 'Content-Type': 'application/json' },
      });
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  /** 再試行の待ち時間を進めながら、クエリが落ち着くまで回す。 */
  async function settle() {
    for (let i = 0; i < 6; i += 1) {
      await flushPromises();
      await vi.advanceTimersByTimeAsync(5_000);
    }
    await flushPromises();
  }

  it('描画に成功したら HTML と、描画に渡した本文を返す', async () => {
    replies = [{ status: 200, html: '<h1>見出し</h1>' }];
    mountHost();
    await settle();

    expect(rendered.data.value).toEqual({ html: '<h1>見出し</h1>', source: DESCRIPTION });
    expect(calls).toBe(1);
  });

  // 入力由来の拒否。その本文では何度試しても通らないので、結果として覚えてよい
  it.each([411, 413, 422])('%i は結果として覚え、再試行しない', async (status) => {
    replies = [{ status }];
    mountHost();
    await settle();

    expect(rendered.data.value).toEqual({ html: null, source: DESCRIPTION });
    expect(rendered.isError.value).toBe(false);
    expect(calls).toBe(1);
  });

  // 回帰ガード。一時的な失敗を html: null の「成功」として覚えると、
  // staleTime: Infinity のぶん分割ビューが素の Markdown のまま固定される
  it.each([401, 500, 503])('%i は成功として覚えず、再試行して回復する', async (status) => {
    replies = [{ status }, { status: 200, html: '<h1>見出し</h1>' }];
    mountHost();
    await settle();

    expect(calls).toBeGreaterThan(1);
    expect(rendered.data.value).toEqual({ html: '<h1>見出し</h1>', source: DESCRIPTION });
    expect(rendered.isError.value).toBe(false);
  });

  it('一時的な失敗が続いたら諦めるが、html: null を覚え込まない', async () => {
    replies = [{ status: 500 }];
    mountHost();
    await settle();

    expect(rendered.isError.value).toBe(true);
    // data を持たないので、呼び出し側は `?? null` でプレーン表示へ倒れる。
    // 同時に「まだ取れていない」ままなので、次のマウント・フォーカスで取り直せる
    expect(rendered.data.value).toBeUndefined();
    expect(calls).toBeGreaterThan(1);
  });

  it('本文が空なら問い合わせない', async () => {
    replies = [{ status: 200, html: '<h1>見出し</h1>' }];
    mountHost(null);
    await settle();

    expect(calls).toBe(0);
    expect(rendered.data.value).toBeUndefined();
  });
});
