/**
 * kfm-code のタグ名 (単一ソース)。
 * emit (rehype プラグイン)・sanitize (SanitizeSchema.tags)・client 登録 (_client-registry)
 * の三点が同じタグを参照する契約 (_client-registry.ts の「三点を揃えること」)。
 * element.ts (client 専用・軽量) と index.ts (rehype 層・SSR 側) の双方から import される
 * ため、どちらの依存も持たない独立ファイルに置く (remark-kfm-mermaid/_tag.ts と同型)。
 */
export const KFM_CODE_TAG = 'kfm-code' as const;
