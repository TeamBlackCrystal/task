# KFM Phase 1 実装ノート（github profile）

task 本文（GitHub issue 形式）のレンダラ **KFM (Koyori Flavored Markdown)** の Phase 1 実装を記す。
Phase 1 の実体は **github profile（複製レンダラ）** = GFM ＋ GitHub alerts ＋ 安全 core。
本書は出荷した実装の説明であり、設計判断の根拠は設計書（別管理）を正とする。

## 構成

```
apps/frontend/src/lib/
  remark-gfm/                  GFM 層の薄いラッパ ＋ GFM 由来 class の sanitize スキーマ
    index.ts
  remark-koyori-alerts/        KFM 拡張第一号: GitHub alerts (> [!NOTE] 等) → callout
    index.ts                   自前 transformer（約 40 行・5 分岐）
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

## パイプライン

```
入力テキスト → remark-parse → remark-gfm → remark-koyori-alerts
  → remark-rehype → rehype-stringify → DOMPurify → HTML 文字列 → <div v-html>
```

- `allowDangerousHtml` は使わない。mdast の生 `html` ノードは remark-rehype 既定で消える。
  プラグインは `data.hName` / `hProperties` の型付き emit のみ行う。
- processor は profile ごとに 1 回だけ build して memoize する。

## GitHub alerts（remark-koyori-alerts）

GitHub 完全互換の境界仕様をテストで固定している:

- 5 種のみ（NOTE / TIP / IMPORTANT / WARNING / CAUTION）・type は case-insensitive
- マーカーは blockquote 先頭行に単独。同一行に後続テキストがあれば通常 blockquote
- ネスト不可（内側は通常 blockquote）・不正 type は通常 blockquote へフォールバック
- 出力: `div.kfm-alert.kfm-alert--{type}` ＋ `p.kfm-alert__title`。inline style は一切出さない
- アイコン・配色は `style.css` の名前空間クラスで当てる（消費側で明示 import するサイドカー方式）

## サニタイズ（_sanitize.ts）

DOMPurify は HTML 構造の allowlist に専念する:

- `FORBID_ATTR: ['style']` — inline style は経路を問わず落とす
- class は既知トークン**完全一致** allowlist（`afterSanitizeAttributes` フック）。
  許可集合は各プラグインが export する `SanitizeSchema` を `createRenderer({ sanitizeSchemas })`
  で合成した registry が単一ソース
- `CUSTOM_ELEMENT_HANDLING` は registry 登録制（Phase 1 は登録タグ空）。
  `allowCustomizedBuiltInElements: false` で `is=""` 経路を封鎖

## キャッシュ（_cache.ts）

- `renderDescription` はモジュール singleton = プロセス全体（SSR では全リクエスト・全 tenant）で
  共有される。この前提で **L1 のキーは入力本文そのもの（full-text）**。ハッシュ化は禁止
  （32-bit ハッシュは誕生日境界 ≈ 77,000 件で衝突し、別 tenant 本文の HTML を返す漏えいになる）
- キー前置部 = pipeline fingerprint ＋ profile ＋ 解決済み content-scope config。
  fingerprint は plugin 列と sanitize スキーマから導出し、構成変更で旧 HTML が自動失効する
- `lru-cache` は `max` ＋ `maxSize` ＋ `sizeCalculation`（UTF-8 バイト長）で有界
- L2（ブラウザ永続）は不採用。必要性を計測してから設計する

## SSR / Hydration 契約

- サーバ生成 HTML を唯一の入力とする。ページの `+data.ts` で
  `descriptionHtml: await renderDescription(text)` を実行して `pageContext.data` に載せ、
  コンポーネントは `v-html` で受けるだけにする
- クライアントは再パース・再サニタイズしない（DOMPurify はサーバで一度だけ）
- カスタム要素の登録は `src/pages/+client.ts`（client 専用 entry）から行い、関数側にも
  `customElements` 不在ガードを持つ二重防御。main.ts は存在せず、`+onCreateApp.ts` は
  SSR でも走るため使わない

## 利用方法

```ts
// +data.ts（サーバ側）
import { renderDescription } from '@/lib/markup-renderer';
const descriptionHtml = await renderDescription(task.description); // 既定 profile = github

// 消費側レイアウトで alert CSS を明示 import
import '@/lib/remark-koyori-alerts/style.css';
```

## 拡張の口（seam）

将来のフレーバー追加は以下の 3 点に閉じる。コア・sanitize・cache・SSR 契約は共有のまま:

1. `_renderer.ts` の `KfmProfile` union にプロファイル名を足す
2. composition root（`index.ts`）の `profiles` にそのプロファイルの remark 層を注入する
3. 追加プラグインの `SanitizeSchema`（class / タグ / 属性）を `sanitizeSchemas` に合流させる

カスタム要素は `_client-registry.ts` の定義配列に追加し、対応プラグインの `SanitizeSchema.tags` /
`attrs` と三点を揃える。多層 config（`_config.ts`）は解決層を重ねる形で拡張する。

## テスト

`src/lib/__tests__/kfm-*.test.ts` の 4 ファイル 48 テスト:

- `kfm-renderer.test.ts` — GFM 基本・alerts 境界・安全 core・決定性・profile fail-closed
- `kfm-sanitize.test.ts` — FORBID style・class 完全一致・XSS 基本・カスタム要素 registry
- `kfm-cache.test.ts` — djb2 衝突ペアの実衝突証明つき full-text キー検証・fingerprint 分離
- `kfm-client-registry.test.ts` — SSR ガード（customElements 不在で no-op）・二重 define 安全

セキュリティ上の要点（inline style 禁止・full-text キー・client ガード）はいずれも
「その規約を破る変更を入れるとテストが落ちる」形で書かれている。
