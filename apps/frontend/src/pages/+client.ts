// Vike の client 専用 entry (https://vike.dev/client)。SSR では実行されない。
// KFM カスタム要素の登録はブラウザのみで行う (main.ts は存在せず、+onCreateApp.ts は
// SSR でも走るためここに置く)。関数内にも customElements 不在ガードがあり二重防御
// (詳細: @/lib/markup-renderer/_client-registry.ts)。現在は kfm-mermaid を登録する。
// import は composition root (@/lib/markup-renderer) からではなく registry へ直接張る:
// root を経由すると createRenderer がモジュール副作用で走り、KFM 一式 (remark / rehype /
// DOMPurify) が tree-shake されず client バンドルへ丸ごと載るため。
import { registerKfmCustomElements } from '@/lib/markup-renderer/_client-registry';

registerKfmCustomElements();
