# KFM Phase 1 実装ノート（github profile）

task 本文（GitHub issue 形式）のレンダラ **KFM (Koyori Flavored Markdown)** の Phase 1 実装を記す。
Phase 1 の実体は **github profile（複製レンダラ）** = GFM ＋ GitHub alerts ＋ 安全 core。
本書は出荷した実装の説明であり、設計判断の根拠は設計書（別管理）を正とする。

**Phase 1 の出荷範囲はレンダラ（`renderDescription`）の提供まで**。UI への接続
（タスク詳細の `+data.ts` から呼び出して `v-html` へ渡す変更と、alert CSS の消費側
import）は本 PR には含めず、別 PR で行う。現時点で `renderDescription` を呼ぶ本番
コードは存在せず、後述の「利用方法」は接続時の使い方を先に示すものである。

## 構成

```
apps/frontend/src/lib/
  remark-gfm/                  GFM 層の薄いラッパ ＋ GFM 由来 class の sanitize スキーマ
    index.ts
    content-class.ts           器クラス kfm-content の単一ソース（GFM CSS の scope）
    style.css                  サイドカー CSS（リスト/blockquote/リンク。preflight 対策。
                               全ルール .kfm-content 子孫限定）
  remark-koyori-alerts/        KFM 拡張第一号: GitHub alerts (> [!NOTE] 等) → callout
    index.ts                   自前 transformer（GitHub alerts の境界規則）
    style.css                  サイドカー CSS（アイコンは名前空間クラス・inline style 不使用）
  markup-renderer/             KFM コア
    index.ts                   composition root（renderDescription singleton・公開 API）
    _renderer.ts               createRenderer（controlled pipeline・profile memoize）
    _sanitize.ts               DOMPurify 設定（構造専任・registry 方式）
    _cache.ts                  L1 キャッシュ（full-text キー）
    _config.ts                 多層 config 解決（Phase 1 はコード既定＋system 層）
    _client-registry.ts        カスタム要素の client 登録（登録タグは現状空）
apps/frontend/src/pages/
  +client.ts                   client 専用 entry（カスタム要素登録の呼び出し口）
```

`index.ts` のみが外部 API（`_*.ts` は内部）。コアはプラグインを import せず、
composition root が remark 層と sanitize スキーマを注入する。
ただし client registry は例外で、composition root から再エクスポートしてはならない。
`+client.ts` は `_client-registry.ts` を直接 import し、サーバ用レンダラ一式が client bundle へ
混入するのを防ぐ。この非再エクスポート規約は bundle 境界であり、公開 API の整理ではない。

## パイプライン

```
入力テキスト → 改行 LF 正規化 → remark-parse → remark-gfm → remark-koyori-alerts
  → remark-rehype → rehype-stringify → DOMPurify → HTML 文字列 → <div v-html>
```

- `allowDangerousHtml` は使わない。mdast の生 `html` ノードは remark-rehype 既定で消える。
  プラグインは `data.hName` / `hProperties` の型付き emit のみ行う。
- processor は profile ごとに 1 回だけ build して memoize する。ただし memoize されるのは
  scope なし（既定 `clobberPrefix`）の経路のみ。scope 付き描画は毎回 build する——scope の
  値空間（comment id 等）は非有界で、singleton に溜めるとメモリが漏れるため
  （`_renderer.ts` の `getProcessor` の分岐）。
- 描画の既定 profile は composition root が `CreateRendererOptions.defaultProfile` へ
  `contentConfig.defaultProfile` を渡して接続する。未指定時の fallback は `github`。

## GitHub alerts（remark-koyori-alerts）

GitHub 完全互換の境界仕様をテストで固定している:

- 5 種のみ（NOTE / TIP / IMPORTANT / WARNING / CAUTION）・type は case-insensitive
- マーカーは blockquote 先頭行に単独。同一行に後続テキストがあれば通常 blockquote
- ネスト不可。alert 内の alert と、通常 blockquote 内の alert の両側とも、内側は通常
  blockquote のままにする。不正 type も通常 blockquote へフォールバックする
- 出力: `div.kfm-alert.kfm-alert--{type}` ＋ `p.kfm-alert__title`。inline style は一切出さない
- アイコン・配色は `style.css` の名前空間クラスで当てる（消費側で明示 import するサイドカー方式）

## サニタイズ（_sanitize.ts）

DOMPurify は HTML 構造の allowlist に専念する:

- `FORBID_ATTR: ['style']` — inline style は経路を問わず落とす
- class は既知トークン**完全一致** allowlist（`afterSanitizeAttributes` フック）。
  許可集合は各プラグインが export する `SanitizeSchema` を `createRenderer({ sanitizeSchemas })`
  で合成した registry が単一ソース
- 動的 class は `SanitizeSchema.classPatterns` で明示した固定形だけを許可する。Phase 1 では
  コードフェンスの `language-*` に限定し、任意のアプリ class を通す汎用パターンにはしない
- `CUSTOM_ELEMENT_HANDLING` は registry 登録制（Phase 1 は登録タグ空）。
  `allowCustomizedBuiltInElements: false` で `is=""` 経路を封鎖

DOMPurify を最終段に置くのは、remark プラグインが emit したものを含む最終 HTML 文字列を、
`v-html` へ渡す直前の一つの境界で検査するためである。class allowlist 用フックは DOMPurify の
モジュール singleton を汚染しないよう sanitize 呼び出し中だけ登録し、`finally` で撤去する。

## キャッシュ（_cache.ts）

- `renderDescription` はモジュール singleton = プロセス全体（SSR では全リクエスト・全 tenant）で
  共有される。この前提で **L1 のキーは入力本文そのもの（full-text）**。ハッシュ化は禁止
  （32-bit ハッシュは誕生日境界 ≈ 77,000 件で衝突し、別 tenant 本文の HTML を返す漏えいになる）
- キー前置部 = pipeline fingerprint ＋ profile ＋ scope ＋ 解決済み content-scope config。
  scope をキーに載せることは安全条件の一部——落とすと `clobberPrefix` の違う HTML を
  取り違え、別断片の脚注 id が付いた HTML を返す。
  fingerprint は plugin 列と sanitize スキーマから導出し、構成変更で旧 HTML が自動失効する
- `lru-cache` は `max` ＋ `maxSize` ＋ `sizeCalculation`（UTF-8 バイト長）で有界
- L2（ブラウザ永続）は不採用。必要性を計測してから設計する

## SSR / Hydration 契約

- サーバ生成 HTML を唯一の入力とする。ページの `+data.ts` で
  `descriptionHtml: await renderDescription(text)` を実行して `pageContext.data` に載せ、
  コンポーネントは `v-html` で受けるだけにする
- 同一ページに複数の KFM 断片（タスク本文＋コメント等）を並べる場合は、断片ごとに
  **決定的な scope** を渡す: `renderDescription(text, { scope: 'comment-42' })`。
  ランダムにしないのは同一入力→同一 HTML（L1 キャッシュ・SSR/CSR 同一性）を保つため。
  scope は `[A-Za-z0-9_-]+` のみ許可し、それ以外は throw する
- 現状 scope が分離するのは脚注の `fn-*` / `fnref-*` 系 id だけで、見出しの
  `footnote-label` と参照側の `aria-describedby` は複数断片でも同じ値になる。この残課題は
  後続 PR #588 の rehype 層で解消するため、本 PR では現状固定試験のみを置く
- 入口で `\r\n` と単独 `\r` を `\n` へ正規化する（キー構築より前）。正規化しないと
  alert のマーカー照合が CRLF 本文で成立せず、LF 版と CRLF 版が別 HTML・
  別キャッシュエントリになる
- クライアントは再パース・再サニタイズしない（DOMPurify はサーバで一度だけ）
- カスタム要素の登録は `src/pages/+client.ts`（client 専用 entry）から行い、関数側にも
  `customElements` 不在ガードを持つ二重防御。main.ts は存在せず、`+onCreateApp.ts` は
  SSR でも走るため使わない

## 利用方法

```ts
// +data.ts（サーバ側）
import { renderDescription } from '@/lib/markup-renderer';
const descriptionHtml = await renderDescription(task.description); // 既定 profile = github

// 消費側レイアウトで alert / GFM CSS を明示 import
import '@/lib/remark-koyori-alerts/style.css';
import '@/lib/remark-gfm/style.css';
```

```html
<!-- 消費側コンポーネント: v-html する器に kfm-content を付ける（GFM CSS の scope） -->
<div class="kfm-content" v-html="descriptionHtml" />
```

二つのサイドカー CSS は消費契約の前提が異なる:

- **alerts CSS は import のみで当たる** — レンダラ自身が名前空間クラス
  （`.kfm-alert` 等）を emit し、CSS がそれを直接指すため器は不要
- **GFM CSS は import ＋ 器クラスの二点契約** — GFM 出力は素の ul/ol/blockquote/a/del
  で掴む class が無く、bare 要素へ当てるとアプリ全体へ漏れるため、全ルールが
  `.kfm-content` 子孫限定。器クラスを付け忘れると一行も当たらない

器クラスの単一ソースは `remark-gfm/content-class.ts`（`KFM_CONTENT_CLASS`）。CSS との
scope 一致は `kfm-gfm-css-contract.test.ts` が強制し、story の器も同じ定数を使う
（VRT baseline の器 = 本番の器）。

## 拡張の口（seam）

将来のフレーバー追加は以下の 3 点に閉じる。コア・sanitize・cache・SSR 契約は共有のまま:

1. `_renderer.ts` の `KfmProfile` union にプロファイル名を足す
2. composition root（`index.ts`）の `profiles` にそのプロファイルの remark 層を注入する
3. 追加プラグインの `SanitizeSchema`（class / タグ / 属性）を `sanitizeSchemas` に合流させる

カスタム要素は `_client-registry.ts` の定義配列に追加し、対応プラグインの `SanitizeSchema.tags` /
`attrs` と三点を揃える。多層 config（`_config.ts`）は解決層を重ねる形で拡張する。
追加プラグインは生 HTML や inline style を出さず、`data.hName` / `hProperties` の型付き emit と
対応する `SanitizeSchema` を同じ変更で提供する。新しい class は完全一致 token を原則とし、
動的な値が必要な場合だけ、許容字種を狭く固定した `classPatterns` と陽性・陰性試験を加える。

## テスト

`src/lib/__tests__/kfm-*.test.ts` の 6 ファイル（件数は追加で変わるため書かない）:

- `kfm-renderer.test.ts` — GFM 基本・alerts 境界・安全 core・決定性・profile fail-closed
- `kfm-sanitize.test.ts` — FORBID style・class 完全一致・XSS 基本・カスタム要素 registry
- `kfm-cache.test.ts` — djb2 衝突ペアの実衝突証明つき full-text キー検証・fingerprint 分離
- `kfm-client-registry.test.ts` — SSR ガード（customElements 不在で no-op）・二重 define 安全
- `kfm-gfm-css-contract.test.ts` — GFM サイドカー CSS の scope が器クラス単一ソースと一致
- `kfm-story-fixtures.test.ts` — story fixture の drift 検査・孤立 rendered/*.html の検出

セキュリティ上の要点（inline style 禁止・full-text キー・client ガード）はいずれも
「その規約を破る変更を入れるとテストが落ちる」形で書かれている。

## story fixture

KFM の Storybook story (`stories/kfm/*`) は本番と同じ「サーバ生成 HTML を v-html するだけ」
の同期描画で VRT baseline を決定的にする。器も本番と同じ `.kfm-content`
（`remark-gfm/content-class.ts` の定数を import）で、GFM サイドカー CSS が本番と同じ条件で
当たる。fixture の運用は次の四点:

- 入力の単一ソースは `src/lib/kfm-story-fixtures/inputs.ts`（キー 1 つ = fixture 1 枚 = story 1 つが基本）
- `rendered/*.html` は `renderDescription` の事前生成物で、手で書き換えない
- 再生成は `pnpm test:unit --update`（`kfm-story-fixtures.test.ts` の `toMatchFileSnapshot` が drift を CI で強制）
- `vite.config.ts` の `fmt.ignorePatterns` から `rendered/**` を外すと drift 検査が偽陽性で落ちる（生成 HTML の整形差分を formatter が触るため）
