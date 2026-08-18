/**
 * _client-registry.ts — KFM カスタム要素の client 登録 seam。
 *
 * 🔴 SSR ガード: task frontend に main.ts は無く、+onCreateApp.ts は SSR でも走る。
 * customElements はブラウザ専用 API のため、登録は src/pages/+client.ts (client 専用
 * entry) から本関数を呼び、さらに関数自身も customElements 不在環境 (Node / SSR) で
 * no-op になる二重ガードとする。ガードを外すと kfm-client-registry テストが落ちる。
 *
 * SSR はカスタム要素を「不活性タグ ＋ light DOM 子」として出力し、client 登録後に
 * ブラウザが upgrade する (Vue 標準挙動)。constructor は factory で遅延させ、
 * HTMLElement が無い環境でモジュール評価が落ちないようにする。
 */
export type KfmCustomElementDefinition = readonly [
  tagName: `kfm-${string}`,
  factory: () => CustomElementConstructor,
];

// Phase 2 seam: kfm-animation / kfm-sparkle はこの配列へ追加する (Phase 1 は登録タグ空)。
// タグを足すときは対応プラグインの SanitizeSchema.tags / attrs にも同じタグを宣言し、
// emit・sanitize・登録の三点を揃えること。
const KFM_CUSTOM_ELEMENTS: readonly KfmCustomElementDefinition[] = [];

export type RegisterKfmCustomElementsResult = {
  /** true = customElements 不在 (SSR / Node) につき何もしなかった */
  readonly skipped: boolean;
  /** 新規に define したタグ数 (登録済みタグは数えない) */
  readonly defined: number;
};

export function registerKfmCustomElements(
  definitions: readonly KfmCustomElementDefinition[] = KFM_CUSTOM_ELEMENTS,
): RegisterKfmCustomElementsResult {
  if (typeof customElements === 'undefined') {
    return { skipped: true, defined: 0 };
  }
  let defined = 0;
  for (const [tagName, factory] of definitions) {
    // 再呼び出し・HMR で define 二重登録例外を出さない
    if (customElements.get(tagName) === undefined) {
      customElements.define(tagName, factory());
      defined += 1;
    }
  }
  return { skipped: false, defined };
}
