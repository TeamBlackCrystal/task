/**
 * GFM サイドカー CSS (style.css) が scope する器のクラス名 (単一ソース)。
 *
 * GFM 出力は素の ul/ol/blockquote/a/del で、GitHub 互換 HTML はこれらに class を
 * 出さない。bare 要素セレクタで当てるとアプリ全体へ漏れるため、style.css の全ルールは
 * このクラスの子孫限定で書く。つまり GFM CSS の消費契約は
 * 「明示 import ＋ v-html する器へこのクラスを付ける」の二点で一つ。
 *
 * 意図的に index.ts からは再エクスポートしない: 器クラスは client コンポーネント側で
 * 使う値であり、root 経由の import は remark-gfm 本体を client バンドルへ載せる経路に
 * なる (markup-renderer の _client-registry を root から再エクスポートしないのと同じ理由)。
 * style.css の scope とこの値の一致は kfm-gfm-css-contract.test.ts が機構として強制する。
 */
export const KFM_CONTENT_CLASS = 'kfm-content';
