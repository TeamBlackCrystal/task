// Vike の client 専用 entry (https://vike.dev/client)。SSR では実行されない。
// KFM カスタム要素の登録はブラウザのみで行う (main.ts は存在せず、+onCreateApp.ts は
// SSR でも走るためここに置く)。関数内にも customElements 不在ガードがあり二重防御
// (詳細: @/lib/markup-renderer/_client-registry.ts)。Phase 1 の登録タグは空 (seam のみ)。
import { registerKfmCustomElements } from '@/lib/markup-renderer';

registerKfmCustomElements();
