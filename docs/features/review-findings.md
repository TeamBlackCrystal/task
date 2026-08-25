---
title: レビュー指摘管理
description: PR レビューの指摘を task で追跡し、GitHub には要約コメント 1 本だけを置く仕組みと運用ルール
icon: lucide:search-check
---

# レビュー指摘管理 仕様書

> ステータス: **Draft** / 作成日: 2026-08-26
> 依存: GitHub 連携（`github_integrations`、1 プロジェクト = 1 リポジトリ）、GitHub App、PAT スコープ

---

## 1. 背景と課題

開発フローは「AI がコードを書く → 別の作業者（AI または人間）がレビュー → 修正 → 再レビュー」
を回しているが、次の 2 つが遅さと散らかりの原因になっている。

1. **Low までマージ前に完璧に直している。** 優先度の低い指摘の修正までマージの前提に
   なっており、1 機能のリードタイムが不必要に伸びる。
2. **GitHub が指摘のデータ置き場になっている。** インラインコメントを 1 指摘 = 1 スレッドで
   積むと PR ページが重くなり、見通しも悪い。実例が
   [#587](https://github.com/koyori-app/task/pull/587) で、スレッド 28 本すべてが
   1 コメントのみ（返信ゼロ）、resolved は 16/28 で管理が途中で崩れている。
   議論の場としても状態管理としても機能していない。
   かといって Low を GitHub Issue に逃すと Issue 一覧が散らかる。

本仕様は、レビュー指摘を task 自身のデータとして管理し（ドッグフーディング）、
GitHub 側には bot の要約コメント 1 本だけを置く形に切り替える。

将来の「AI がレビューを検知して自動修正するループ」は**本仕様の範囲外**だが、
その土台（機械可読な指摘・状態・集計）になるようにモデルを設計する（§9）。

---

## 2. 運用ルール

コードを書く前に決まる規約。**task と vrt の両リポジトリに適用する。**

| ルール | 内容 |
|---|---|
| マージ基準 | **High / Medium はマージ前に必須。Low / Nit は繰り延べ可**（deferred → 通常タスク化して後日対応） |
| インライン禁止 | GitHub の PR へインラインレビューコメントを投稿しない。PR に置くのは bot の**要約コメント 1 本**のみ |
| 権威の所在 | 指摘の一覧・状態の唯一の権威は task。GitHub のスレッド resolved フラグは使わない・見ない |
| 再レビュー | 依頼ごとに 1 巡。判定は verified（解消）/ 据え置き / 未対応 で、指摘の状態として記録する |

---

## 3. 概念モデル

```
project（GitHub 連携済み）
└── reviews（レビュー巡: 1 PR への 1 回のレビュー）
    └── review_findings（指摘）
```

### reviews（レビュー巡）

| 項目 | 内容 |
|---|---|
| `project_id` | 対象プロジェクト。プロジェクトの GitHub 連携先リポジトリの PR を対象とする |
| `pr_number` | PR 番号 |
| `head_sha` | レビュー時点の PR head（裏取りした commit の記録） |
| `reviewer_id` | レビュー巡を作成した利用者（PAT の持ち主。AI もこの利用者として動く） |
| `summary` | 総評（markdown） |

同一 PR への再レビューは**新しい巡**として作る（更新しない）。「どの巡で出た指摘か」
「どの head を見たか」が履歴として残る。

### review_findings（指摘）

| 項目 | 内容 |
|---|---|
| `review_id` | 属するレビュー巡 |
| `severity` | `high` / `medium` / `low` / `nit` |
| `title` | 1 行の要約 |
| `body` | 詳細（markdown。再現条件・根拠を書く） |
| `file` / `line` | 位置情報（任意。インラインコメントの代替はこのテキスト情報で足りる） |
| `state` | `open` / `fixed` / `verified` / `deferred` / `rejected` |
| `deferred_task_id` | 繰り延べ時に自動起票した通常タスクへのリンク（任意） |

### 状態遷移

```
open ──→ fixed ──→ verified        （修正宣言 → レビュー側の確認）
  │        │
  │        └─→ deferred            （直したが確認は後回し、は作らない。fixed からの繰延は不可）
  ├─→ deferred                     （Low/Nit を繰り延べ。同プロジェクトに通常タスクを自動起票しリンク）
  └─→ rejected                     （指摘自体が誤り。レビュー側だけが遷移できる）
```

- `fixed` へは `write:review` を持つ誰でも遷移できる（修正側の宣言）
- `verified` / `rejected` へは**レビュー側だけ**が遷移できる: その指摘を含む巡の作成者、
  または同じ PR のより新しい巡の作成者。**`fixed` を宣言した本人は不可**
  （自分の修正を自分で検証済みにできない）
- `deferred` にした時点で、同じプロジェクトへ通常タスク（優先度 Low、本文に指摘への参照）を
  自動起票して `deferred_task_id` にリンクする。以降の追跡は普段のタスク運用に乗る
- 各遷移は誰がいつ行ったかを記録する（監査ログと同じ流儀）

---

## 4. 認可

- 新スコープ **`read:review` / `write:review`** を追加する。レビュー専用の AI に
  タスク書き換え権限（`write:task` 等）を渡さずに済ませるため
- `admin:tenant` は既存規約どおり全スコープを包含する
- リソース束縛は既存の PAT 規則（テナント束縛 + `allowed_project_ids`）にそのまま乗る
- セッションは既存どおり全スコープ相当。閲覧はプロジェクトに入れる人全員、
  作成・遷移は §3 の役割規則に従う

---

## 5. API（概要）

パスは既存規約どおりテナント・プロジェクト配下に置く。

| メソッドとパス | 動作 |
|---|---|
| `POST /v1/tenants/{t}/projects/{p}/reviews` | レビュー巡 + 指摘の**一括作成**（1 リクエスト）。成功後に GitHub 要約更新をジョブ投入 |
| `GET  /v1/tenants/{t}/projects/{p}/reviews?pr=618` | 巡の一覧（指摘の件数つき） |
| `GET  /v1/tenants/{t}/projects/{p}/reviews/{id}` | 巡の詳細（指摘含む） |
| `PATCH /v1/tenants/{t}/projects/{p}/review-findings/{id}` | 状態遷移（`state` と任意のコメント） |
| `GET  /v1/tenants/{t}/projects/{p}/reviews/summary?pr=618` | PR 単位の集計: 重大度 × 状態の件数と「マージ可否」（open/fixed の High・Medium が 0 か） |

- PR 番号は数値としてのみ検証する（実在確認は要約コメント投稿時に判明する。
  投稿失敗は起票を巻き戻さない）
- 状態遷移後も要約更新ジョブを投入する

---

## 6. CLI

主経路は **JSON 一括投入**（AI が生成しやすく、検証もしやすい）。

```bash
# レビュー 1 巡ぶんを一括起票（ファイル or stdin）
task review submit --project TASK --pr 618 findings.json

# 指摘一覧（フィルタつき）
task review list --project TASK --pr 618 --state open --severity high,medium

# 状態遷移
task review resolve <finding-id> --state fixed
task review resolve <finding-id> --state deferred   # 通常タスクの自動起票込み

# 集計。High/Medium が残っていれば非 0 で終了する（CI や手元のマージ前確認に使える）
task review summary --project TASK --pr 618
```

### submit の JSON スキーマ（例）

```json
{
  "pr": 618,
  "head_sha": "60cdd7795f94fa4e4148ce996c2efb4c363e3f5e",
  "summary": "総評。実装は整合。ストーリーのセレクタに 1 件。",
  "findings": [
    {
      "severity": "medium",
      "title": "LastAdminGuard の findByRole が複数一致で必ず失敗する",
      "body": "説明文にもマッチするため…（再現条件・根拠）",
      "file": "apps/frontend/stories/components/MembersSection.stories.ts",
      "line": 257
    }
  ]
}
```

指摘ゼロ（`findings: []`）の巡も正当（「指摘なし」の記録として意味を持つ）。

---

## 7. GitHub 要約コメント

- 投稿主体は **GitHub App**（既存連携の installation token）。個人の PAT に依存しない
- 1 PR に 1 本。HTML マーカー（例: `<!-- koyori-review-summary -->`）で自分のコメントを
  特定し、以後は**同じコメントを編集**して更新する
- 内容: 巡数と最新巡の総評、重大度 × 状態の件数表、マージ可否、task の指摘一覧への
  リンク、最終更新時刻
- 更新契機: 巡の作成時と指摘の状態遷移時（apalis ジョブで非同期。ペイロードに
  機微情報を載せない既存規約に従う）
- GitHub 連携の無いプロジェクトでは投稿をスキップする（起票・管理は可能）
- 投稿・編集の失敗はベストエフォート（ログに残すが API は成功させる）

---

## 8. UI 設計へのインプット

画面は本仕様の範囲外（Claude Designer で別途設計する）。設計に渡すケーパビリティ要約:

- **PR 単位の指摘一覧**: 重大度・状態・巡でフィルタ。各指摘は title / file:line / 本文 /
  遷移履歴を持つ
- **状態遷移の操作**: fixed / verified / deferred / rejected。役割制約あり
  （自分の修正を自分で verified にできない）
- **マージ可否の即答**: High/Medium の残数が 0 かどうか（バッジ 1 個で表せる情報）
- **巡の履歴**: 同じ PR に何巡レビューが走り、どの head を見たか
- **繰り延べの行き先**: deferred → リンクされた通常タスクへジャンプ
- **プロジェクト横断の集計**: 溜まっている deferred（Low/Nit）の件数と一覧

---

## 9. 範囲外（今回やらない）

| 項目 | 理由・メモ |
|---|---|
| AI 自動修正ループ（検知 → 修正 → 報告） | 別仕様。導入時は **ラウンド上限（自動往復 1 巡まで）/ 重大度ゲート（High の自動修正は人間必須）/ 収束判定（同一箇所への再指摘で停止）** の 3 ガードレールを必須とする。本モデルの巡・状態・集計はその前提を満たす |
| GitHub インラインコメントの取り込み・同期 | resolved 状態の権威が二重になる沼を避ける。#587 型の過去データも移行しない |
| 画面 | §8 のケーパビリティ要約を入力に Claude Designer で設計する |

---

## 10. 受け入れ条件（テスト観点）

- 正常系: 一括起票 → 一覧 → fixed → verified が通り、GitHub に要約コメントが
  **1 本だけ**作られ、状態遷移で同じコメントが更新される
- 指摘ゼロの巡、同一 PR への 2 巡目（新しい巡になる）、`findings` が境界を越える件数
- 拒否系: スコープ不足 403 / 他テナント・他プロジェクト 404 / fixed 宣言者本人による
  verified の拒否（対照: 別レビュワーなら成功）/ rejected 済みへの再遷移
- 繰り延べ: deferred で通常タスクが同プロジェクトに起票されリンクされる。
  タスク起票に失敗したとき deferred へ遷移しない（不整合を作らない）
- 要約コメント: 連携なしプロジェクトでは投稿だけスキップして起票は成功。
  投稿失敗は起票を巻き戻さずログに残る

---

## 11. 決定事項ログ

- 2026-08-26: 範囲は「運用ルール + 指摘のタスク管理 + GitHub 要約投稿」。自動修正ループは範囲外
- 2026-08-26: データモデルは専用エンティティ（reviews / review_findings）。将来の自動化が
  機械可読な状態・集計を要求するため、既存タスク + カスタムフィールドの相乗りは不採用
- 2026-08-26: 重大度は High / Medium / Low / Nit の 4 段階。マージ必須は High / Medium
- 2026-08-26: 状態は open / fixed / verified / deferred / rejected。verified / rejected は
  レビュー側のみ、fixed 宣言者本人は不可
- 2026-08-26: 起票はレビュワーが CLI から JSON 一括投入。GitHub 経由の取り込みはしない
- 2026-08-26: 置き場所は GitHub 連携済みプロジェクト。deferred は通常タスクへ自動変換
- 2026-08-26: GitHub へは App（bot）名義の要約コメント 1 本のみ。インライン投稿は禁止
- 2026-08-26: スコープは read:review / write:review を新設
