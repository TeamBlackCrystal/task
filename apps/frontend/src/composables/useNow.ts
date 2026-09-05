import { computed, onScopeDispose, ref } from 'vue';

/**
 * 一定間隔で進む現在時刻。
 *
 * 相対時刻（「たった今」「1分前」）をテンプレートから `new Date()` で作ると
 * リアクティブに追跡されないため、画面を開いたままだと表示が止まる。
 * 時刻そのものを ref にして描画に載せる。
 */
export function useNow(intervalMs = 30_000) {
  const now = ref(new Date());

  // SSR ではタイマーを張らない（1 回描画したら進める先が無い）
  if (typeof window !== 'undefined') {
    const timer = window.setInterval(() => {
      now.value = new Date();
    }, intervalMs);
    onScopeDispose(() => window.clearInterval(timer));
  }

  return computed(() => now.value);
}
