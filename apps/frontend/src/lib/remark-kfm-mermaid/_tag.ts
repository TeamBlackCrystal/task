/**
 * kfm-mermaid のタグ名 (単一ソース)。
 * emit (remark プラグイン)・sanitize (SanitizeSchema.tags)・client 登録 (_client-registry)
 * の三点が同じタグを参照する契約 (_client-registry.ts の「三点を揃えること」)。
 * element.ts (client 専用・軽量) と index.ts (remark 層・SSR 側) の双方から import される
 * ため、どちらの依存も持たない独立ファイルに置く (client entry が remark 層を巻き込まない)。
 */
export const KFM_MERMAID_TAG = 'kfm-mermaid' as const;
