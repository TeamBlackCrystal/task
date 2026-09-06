---
title: タスク機能 概要
description: タスク管理システムの全体像・機能依存関係・実装順序
icon: lucide:layout-list
---

# タスク機能 概要

> 作成日: 2026-05-27

仕様書は機能グループ単位に分割されている。各ファイルが 1 PR に対応する。

---

## 機能一覧と仕様書リンク

| # | 機能グループ | 仕様書 | 依存 | 状態 |
|---|------------|--------|------|------|
| 1 | **コア**（CRUD・ステータス・担当者・ラベル・マイルストーン・依存関係） | [tasks/core](/features/tasks/core) | なし | ✅ 完了 ([#40](https://github.com/TeamBlackCrystal/task/pull/40)) |
| 2 | **作業時間追跡**（タイマー・手動ログ） | [tasks/time-tracking](/features/tasks/time-tracking) | 1 | 未着手 |
| 3 | **スプリント**（イテレーション・バーンダウン） | [tasks/sprints](/features/tasks/sprints) | 1 | 未着手 |
| 4 | **コメント・アクティビティ**（スレッド・変更履歴・@メンション） | [tasks/collaboration](/features/tasks/collaboration) | 1 | 未着手 |
| 5 | **通知・ウォッチャー**（in-app・メール・購読） | [tasks/notifications](/features/tasks/notifications) | 1, 4 | 未着手 |
| 6 | **カスタムフィールド**（プロジェクト固有属性） | [tasks/custom-fields](/features/tasks/custom-fields) | 1 | 未着手 |
| 7 | **検索・バルク・ビュー・添付**（全文検索・一括更新・保存フィルター・Drive連携） | [tasks/extensions](/features/tasks/extensions) | 1, (Drive) | 未着手 |
| 8 | **自動化**（トリガー→アクションエンジン） | [tasks/automation](/features/tasks/automation) | 1, 4, 5 | 未着手 |
| 9a | **GitHub App 基盤**（インストール・認証情報管理・Webhook 受信インフラ） | [tasks/github-app](/features/tasks/github-app) | なし（PR 1 と並行可） | ✅ 完了 ([#44](https://github.com/TeamBlackCrystal/task/pull/44)) |
| 9b | **Git ホスティング↔タスク連携**（PR・コミット・ブランチのリンク・自動クローズ・コミットのアクティビティ表示。GitLab / Forgejo を見据えたホスト中立モデル） | [tasks/github-tasks](/features/tasks/github-tasks) | 1, 8, 9a, レビュー指摘 | 未着手（仕様改訂 2026-09-04） |
| 10 | **Webhook**（外部向けイベント送信） | [tasks/webhooks](/features/tasks/webhooks) | 1 | 未着手 |
| — | **フロントエンド** | 各仕様書末尾の「Phase B」節 | 1〜10 すべて完了後 | 未着手 |

---

## 依存関係図

```text
[1 コア] ──────────────────────────────────────────┐
    │                                               │
    ├──[2 作業時間追跡]                              │
    │                                               │
    ├──[3 スプリント]                                │
    │                                               │
    ├──[4 コメント・アクティビティ]──[5 通知・ウォッチャー]
    │                                          │
    ├──[6 カスタムフィールド]                   │
    │                                          │
    ├──[7 検索・バルク・ビュー・添付]            │
    │                                          ↓
    ├──[8 自動化] ←──────────────────────[4, 5]
    │       │
    │       └──[9b GitHub↔タスク連携] ←──[8, 9a]
    │
    ├──[9a GitHub App 基盤]（並行可）
    │
    └──[10 Webhook]（並行可）
```

---

## 実装順序ルール

1. **バックエンド完全完了後にフロントエンドへ移行する**（並行実装禁止）
2. 各 PR は依存 PR がマージ済みであることを前提とする
3. 依存関係のない PR（2・3・6・9a・10）は並行して進めてよい
4. フロントエンドは全バックエンド PR がマージされてから `pnpm openapi` で型を再生成して着手

---

## 共通仕様

### スコープ（全 PR 共通）

```rust
pub enum Scope {
    ReadTask, WriteTask,
    ReadMilestone, WriteMilestone,
    ManageWebhook,
    ManageAutomation,
    ManageGitHub,
}
```

### URL 規則

- プロジェクトスコープのリソース: `/v1/tenants/{tenant_id}/projects/{project_id}/...`
- ユーザースコープのリソース: `/v1/users/me/...`
- システムエンドポイント（GitHub Webhook 受信など）: `/v1/...`

### 優先順位 enum

| DB 値 | 表示 | 色 |
|-------|------|-----|
| `critical_fire` | 🔥 炎上 | 赤（点滅） |
| `critical` | 🚨 Critical | 赤 |
| `high` | 🔴 High | オレンジ |
| `medium` | 🟡 Medium | 黄 |
| `low` | 🟢 Low | 緑 |
| `trivial` | ⚪ 雑魚 | グレー |

### タスク連番 ID

プロジェクト内で `ENG-1`, `ENG-2`, ... の連番を振る（`KEY` はプロジェクトキー）。`project_task_counters` テーブルで `SELECT ... FOR UPDATE` によりアトミックに採番。API の `GET /tasks/{id}` は UUID と `KEY-N` どちらでも受け付ける。GitHub の `#N`（Issue 番号）と区別するためこの形式を採用。
