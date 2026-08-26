# フロントエンド URL 仕様書

> ステータス: **Draft**
> 作成日: 2026-05-31
> Vike route parameter naming: **camelCase**（例: `@folderId`, `@projectKey`, `@taskId`）

---

## 1. 識別子の対応

バックエンド API の UUID ベースパスをフロントエンドでは人間可読な識別子に置き換える。

| リソース | バックエンド | フロントエンド | 例 |
|---------|------------|--------------|-----|
| テナント | UUID (`tenant_id`) | `display_id`（スラグ） | `acme` |
| プロジェクト | UUID (`project_id`) | `key`（`^[A-Z][A-Z0-9]{1,9}$`） | `ENG` |
| タスク | UUID または `KEY-N` | `KEY-N` 形式 | `ENG-42` |
| ドライブフォルダ | UUID | UUID（スラグなし） | `3fa85f64-...` |

> タスクの `KEY-N` 形式は `GET /tasks/{id}` API が UUID と両対応しているため、フロントエンドからそのまま渡せる（`seq_id` + `project.key` で構成）。

---

## 2. URL 一覧

### 認証（テナント外）

| URL | 説明 |
|-----|------|
| `/signin` | ログイン |
| `/signup` | 新規登録 |
| `/forgot-password` | パスワードリセット申請 |
| `/reset-password` | パスワードリセット実行 |

### アカウント設定（テナント外）

個人アカウントの設定。テナントに紐づかないため `/{tenant}` の外に置く。

| URL | 説明 |
|-----|------|
| `/settings/profile` | プロフィール編集（[仕様](../features/account-settings.md)） |

### テナントスコープ

| URL | 説明 |
|-----|------|
| `/{tenant}` | テナントホーム（プロジェクト一覧 or ダッシュボード） |
| `/{tenant}/settings` | テナント設定 |
| `/{tenant}/members` | テナントメンバー管理 |
| `/{tenant}/drive` | ドライブ（ファイルブラウザ ルート） |
| `/{tenant}/drive/{folder_id}` | ドライブ フォルダ |
| `/{tenant}/projects` | プロジェクト一覧 |

### プロジェクトスコープ

`{key}` はプロジェクトキー（例: `ENG`）、`{KEY-N}` はタスク連番（例: `ENG-42`）。

| URL | 説明 |
|-----|------|
| `/{tenant}/projects/{key}` | プロジェクトホーム（カンバン） |
| `/{tenant}/projects/{key}/tasks` | タスク一覧（カンバン / リスト / テーブル切替） |
| `/{tenant}/projects/{key}/tasks/{KEY-N}` | タスク詳細 |
| `/{tenant}/projects/{key}/milestones` | マイルストーン一覧 |
| `/{tenant}/projects/{key}/labels` | ラベル管理 |
| `/{tenant}/projects/{key}/members` | プロジェクトメンバー |
| `/{tenant}/projects/{key}/settings` | プロジェクト設定 |

### 管理者（`is_admin = true` のユーザーのみ）

| URL | 説明 |
|-----|------|
| `/admin` | 管理ダッシュボード |
| `/admin/users` | ユーザー管理 |
| `/admin/tenants` | テナント管理 |
| `/admin/audit-logs` | 監査ログ閲覧 |

---

## 3. Vike ページディレクトリ構造

```plaintext
apps/frontend/src/pages/
├── +Layout.vue                          # ルートレイアウト（認証チェック）
├── index/+Page.vue                      # / → /{tenant} へリダイレクト or ランディング
├── signin/+Page.vue                     # /signin
├── signup/+Page.vue                     # /signup
├── forgot-password/+Page.vue            # /forgot-password
├── reset-password/+Page.vue             # /reset-password
│
├── settings/
│   └── profile/+Page.vue                # /settings/profile
│
├── @tenant/
│   ├── +Page.vue                        # /{tenant}
│   ├── +Layout.vue                      # テナントレイアウト（共通サイドバー）
│   ├── settings/+Page.vue               # /{tenant}/settings
│   ├── members/+Page.vue                # /{tenant}/members
│   ├── drive/
│   │   ├── +Page.vue                    # /{tenant}/drive
│   │   └── @folderId/+Page.vue          # /{tenant}/drive/{folder_id}（Vike: camelCase）
│   └── projects/
│       ├── +Page.vue                    # /{tenant}/projects
│       └── @projectKey/
│           ├── +Page.vue                # /{tenant}/projects/{key}（カンバン）
│           ├── +Layout.vue              # プロジェクトレイアウト（プロジェクトナビ）
│           ├── tasks/
│           │   ├── +Page.vue            # /{tenant}/projects/{key}/tasks
│           │   └── @taskId/+Page.vue    # /{tenant}/projects/{key}/tasks/{KEY-N}（Vike: camelCase）
│           ├── milestones/+Page.vue     # /{tenant}/projects/{key}/milestones
│           ├── labels/+Page.vue         # /{tenant}/projects/{key}/labels
│           ├── members/+Page.vue        # /{tenant}/projects/{key}/members
│           └── settings/+Page.vue       # /{tenant}/projects/{key}/settings
│
└── admin/
    ├── +Page.vue                        # /admin
    ├── users/+Page.vue                  # /admin/users
    ├── tenants/+Page.vue                # /admin/tenants
    └── audit-logs/+Page.vue             # /admin/audit-logs
```

---

## 4. 設計判断

| 項目 | 決定 | 理由 |
|------|------|------|
| テナント識別子 | `display_id`（スラグ） | UUID より可読性が高い。`acme.example.com` スタイルと同様 |
| プロジェクト識別子 | `key`（大文字英数） | バックエンドで設計済み（`ENG` / `BACK` 等）。URLに映えて一意 |
| タスク識別子 | `KEY-N` 形式 | API が直接サポート。Jira ライクで直感的（`ENG-42`） |
| `/projects/` セグメント保持 | する | `/{tenant}/settings` 等テナント固定ルートと大文字 key が衝突しないよう分離 |
| ドライブフォルダ識別子 | UUID | フォルダにスラグなし。パス式（`/a/b/c`）は Phase 2 以降で検討 |
| 管理者ページ | `/admin/*` に分離 | `is_admin` フラグで保護。テナントスコープとは独立 |

---

## 5. URL ↔ Vike ページファイル 対応表

> **レビュー時チェックリスト**: 新しい URL を追加・変更する際は、この表を同時に更新すること。
> URL パターンとページファイルパスが常に 1:1 対応していることを PR レビューで確認する。

| URL パターン | Vike ページファイル |
|-------------|-------------------|
| `/` | `index/+Page.vue` |
| `/signin` | `signin/+Page.vue` |
| `/signup` | `signup/+Page.vue` |
| `/forgot-password` | `forgot-password/+Page.vue` |
| `/reset-password` | `reset-password/+Page.vue` |
| `/settings/profile` | `settings/profile/+Page.vue` |
| `/{tenant}` | `@tenant/+Page.vue` |
| `/{tenant}/settings` | `@tenant/settings/+Page.vue` |
| `/{tenant}/members` | `@tenant/members/+Page.vue` |
| `/{tenant}/drive` | `@tenant/drive/+Page.vue` |
| `/{tenant}/drive/{folder_id}` | `@tenant/drive/@folderId/+Page.vue` |
| `/{tenant}/projects` | `@tenant/projects/+Page.vue` |
| `/{tenant}/projects/{key}` | `@tenant/projects/@projectKey/+Page.vue` |
| `/{tenant}/projects/{key}/tasks` | `@tenant/projects/@projectKey/tasks/+Page.vue` |
| `/{tenant}/projects/{key}/tasks/{KEY-N}` | `@tenant/projects/@projectKey/tasks/@taskId/+Page.vue` |
| `/{tenant}/projects/{key}/milestones` | `@tenant/projects/@projectKey/milestones/+Page.vue` |
| `/{tenant}/projects/{key}/labels` | `@tenant/projects/@projectKey/labels/+Page.vue` |
| `/{tenant}/projects/{key}/members` | `@tenant/projects/@projectKey/members/+Page.vue` |
| `/{tenant}/projects/{key}/settings` | `@tenant/projects/@projectKey/settings/+Page.vue` |
| `/admin` | `admin/+Page.vue` |
| `/admin/users` | `admin/users/+Page.vue` |
| `/admin/tenants` | `admin/tenants/+Page.vue` |
| `/admin/audit-logs` | `admin/audit-logs/+Page.vue` |

> すべてのパスは `apps/frontend/src/pages/` 配下の相対パス。

---

## 6. 移行メモ

現在存在する仮ページ（`/labels`、`/tasks`）はこの仕様のプロジェクトスコープパスへ移行する。

| 現状 | 移行先 |
|------|--------|
| `/labels` | `/{tenant}/projects/{key}/labels` |
| `/tasks` | `/{tenant}/projects/{key}/tasks` |
