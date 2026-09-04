---
title: Drive 機能仕様
description: Misskey ライクなドライブ機能の設計仕様書
icon: lucide:hard-drive
---

# Drive 機能仕様書

> ステータス: **Draft — 仕様更新中**
> 作成日: 2026-05-26
> 最終更新: 2026-05-26（アクセス制御追加）

---

## 1. 概要

タスク管理 SaaS に Misskey ライクなドライブ機能を追加する。
ドライブはテナント単位のファイル管理スペースであり、アップロードしたファイルをフォルダで整理し、タスクへの添付や共有 URL 発行に利用できる。

ストレージバックエンドは **S3 互換（AWS S3 / MinIO）** と **ローカルディスク** の 2 種類をサポートし、環境変数で切り替える。

---

## 2. スコープ

### Phase 1（MVP）— 本仕様書の対象

| 機能 | 内容 |
|------|------|
| ファイルアップロード | multipart/form-data で受付。S3 または ローカルへ保存 |
| ファイル一覧・取得 | テナント内ファイルの一覧取得、メタデータ取得 |
| ファイル削除 | ストレージ上のオブジェクトも同時削除 |
| フォルダ CRUD | 階層フォルダの作成・取得・削除 |
| 直接 URL | ローカルストレージの場合はバックエンド経由でファイルを配信 |
| フロントエンド UI | ファイルブラウザ、アップロード UI（shadcn/ui） |

### Phase 2 以降（本仕様外）

- ファイルをタスクへ添付
- 画像サムネイル生成
- ファイル全文検索
- フォルダ共有の `editor` 権限（アップロード・削除）
- クォータ超過テナントの管理者向け監査 UI

---

## 3. データモデル

### 3.1 `drive_folders` テーブル

```rust
// entities/drive_folders.rs
pub struct Model {
    pub id: Uuid,
    pub name: String,
    pub parent_id: Option<Uuid>,   // 自己参照（ルートフォルダは None）
    pub tenant_id: Uuid,
    pub project_id: Option<Uuid>,  // プロジェクト紐付き（設定時はプロジェクトフォルダ）
    pub created_by: Uuid,          // FK → users
    pub created_at: DateTimeUtc,
}
```

| カラム | 型 | 制約 | 説明 |
|--------|-----|------|------|
| `id` | UUID | PK | |
| `name` | VARCHAR | NOT NULL | フォルダ名 |
| `parent_id` | UUID | NULLABLE, FK→self | 親フォルダ（ルートは NULL） |
| `tenant_id` | UUID | NOT NULL, FK→tenants CASCADE | |
| `project_id` | UUID | NULLABLE, FK→projects CASCADE | セットされているとプロジェクトフォルダ |
| `created_by` | UUID | NOT NULL, FK→users | |
| `created_at` | TIMESTAMPTZ | NOT NULL DEFAULT now() | |

> **プロジェクトフォルダの自動作成**: プロジェクト作成時に同名フォルダを `drive_folders` に自動作成し、`project_id` を紐付ける。プロジェクト削除時は CASCADE で削除される。

### 3.2 `drive_files` テーブル

```rust
// entities/drive_files.rs
pub struct Model {
    pub id: Uuid,
    pub name: String,              // 表示名（元ファイル名）
    pub size: i64,                 // バイト数
    pub mime_type: String,         // application/octet-stream 等
    pub storage_type: StorageType, // enum: s3 | local
    pub storage_key: String,       // S3 key または ローカル相対パス
    // url カラムなし — API レスポンス時に /v1/drive/files/{id}/content を生成
    pub tenant_id: Uuid,
    pub project_id: Option<Uuid>,  // 非正規化。フォルダの project_id を引き継ぐ
    pub uploader_id: Uuid,         // FK → users
    pub folder_id: Option<Uuid>,   // FK → drive_folders
    pub created_at: DateTimeUtc,
}
```

| カラム | 型 | 制約 | 説明 |
|--------|-----|------|------|
| `id` | UUID | PK | |
| `name` | VARCHAR | NOT NULL | ファイル表示名 |
| `size` | BIGINT | NOT NULL | バイト数 |
| `mime_type` | VARCHAR | NOT NULL | |
| `storage_type` | ENUM | NOT NULL | `s3` または `local` |
| `storage_key` | VARCHAR | NOT NULL | ストレージ固有キー（UUID v4）|
| `tenant_id` | UUID | NOT NULL, FK→tenants CASCADE | |
| `project_id` | UUID | NULLABLE | フォルダの `project_id` を非正規化コピー。アクセス制御の高速判定に使用 |
| `uploader_id` | UUID | NOT NULL, FK→users | |
| `folder_id` | UUID | NULLABLE, FK→drive_folders SET NULL | |
| `created_at` | TIMESTAMPTZ | NOT NULL DEFAULT now() | |

**CHECK 制約**:

```sql
CHECK (project_id IS NULL OR folder_id IS NOT NULL)
```
`project_id` がセットされているときは必ず `folder_id` も非 NULL でなければならない。DB レベルと API ハンドラ（バリデーション層）の両方で強制する。

> **`url` カラムなし**: 全ファイルのアクセス URL は `/v1/drive/files/{id}/content` に統一。DB には `storage_key` のみ保持し、API レスポンス生成時に URL を組み立てる。S3 エンドポイント変更やローカルサーバー移転の影響を受けない。

> **`project_id` の非正規化**: アクセス制御を毎回フォルダ階層を辿らず O(1) で判定するため、ファイルにもフォルダの `project_id` を保持する。ファイルのフォルダ移動時は `project_id` を再設定する。

### 3.3 `drive_folder_shares` テーブル

フォルダの共有設定を管理する。ユーザー指定共有と公開リンク共有の 2 種類をサポートする。

```rust
// entities/drive_folder_shares.rs
pub struct Model {
    pub id: Uuid,
    pub folder_id: Uuid,                 // FK → drive_folders CASCADE
    pub shared_with_user_id: Option<Uuid>, // ユーザー指定共有（NULL = 公開リンク共有）
    pub share_token: Option<String>,     // 公開リンク用トークン（NULL = ユーザー指定共有）
    pub permission: SharePermission,     // enum: viewer | editor
    pub created_by: Uuid,                // FK → users
    pub expires_at: Option<DateTimeUtc>, // 有効期限（NULL = 無期限）
    pub created_at: DateTimeUtc,
}
```

| カラム | 型 | 制約 | 説明 |
|--------|-----|------|------|
| `id` | UUID | PK | |
| `folder_id` | UUID | NOT NULL, FK→drive_folders CASCADE | 共有対象フォルダ |
| `shared_with_user_id` | UUID | NULLABLE, FK→users | 特定ユーザーへの共有。NULL の場合は公開リンク |
| `share_token` | VARCHAR | NULLABLE, UNIQUE | 公開リンク用トークン（URL-safe ランダム文字列）。NULL の場合はユーザー指定共有 |
| `permission` | ENUM | NOT NULL | `viewer`（閲覧のみ）または `editor`（アップロード・削除も可） |
| `created_by` | UUID | NOT NULL, FK→users | 共有を作成したユーザー |
| `expires_at` | TIMESTAMPTZ | NULLABLE | 有効期限。NULL = 無期限 |
| `created_at` | TIMESTAMPTZ | NOT NULL DEFAULT now() | |

> **制約**: `shared_with_user_id` と `share_token` はどちらか一方のみ設定される（CHECK 制約）。両方 NULL または両方 NOT NULL は不正。

**共有の適用範囲**: フォルダを共有すると、その配下の**サブフォルダ・ファイルすべて**にアクセス権が及ぶ（再帰的に継承）。

### 3.4 `tenants` テーブルへの追加カラム

既存テーブルに以下のカラムを追加する。

| カラム | 型 | 制約 | 説明 |
|--------|-----|------|------|
| `drive_quota_bytes` | BIGINT | NULLABLE | テナントのドライブ最大容量（バイト）。`NULL` = システムデフォルト適用 |

```rust
// entities/tenants.rs に追加
pub drive_quota_bytes: Option<i64>, // NULL = システムデフォルト（DRIVE_DEFAULT_QUOTA_MB 参照）
```

---

## 4. アクセス制御

### 4.1 アクセスルール

| ファイルの種類 | 閲覧できる人 |
|--------------|------------|
| `project_id = NULL`（一般ファイル） | リンクを知っていれば誰でも（認証不要） |
| `project_id = <id>`（プロジェクトファイル） | ① そのプロジェクトに入れる人 ② テナントオーナー ③ フォルダ共有権限を持つユーザー/トークン |

①は「ファイルの `tenant_id` に所属していること」を先に確認したうえで、プロジェクトの公開規則で判定する。
ファイル ID だけで引ける配信経路があるため、プロジェクト所属だけを見るとテナント境界を越えられる。
判定の詳細は [テナント / プロジェクト認可](../../apps/backend/docs/tenant-project-authz.md) を参照。

③のフォルダ共有は所属とは独立した明示的な付与なので、テナントから外しても失効しない。

### 4.2 アクセス判定ロジック

```rust
// アクセス可否チェックの擬似コード
fn can_access_file(file: &DriveFile, caller: &Caller) -> bool {
    // 一般ファイルは誰でもアクセス可
    if file.project_id.is_none() {
        return true;
    }

    // 認証済みユーザーの場合
    if let Caller::User(user) = caller {
        // テナントオーナー OR（テナントに所属 AND プロジェクトの公開規則を満たす）
        if is_tenant_owner(user.id, file.tenant_id)
            || can_access_project(user.id, file.tenant_id, file.project_id)
        {
            return true;
        }
        // フォルダ共有（ユーザー指定）でアクセス権あり
        if let Some(folder_id) = file.folder_id {
            if has_user_share(user.id, folder_id) {
                return true;
            }
        }
    }

    // 公開リンクトークンの場合
    if let Caller::ShareToken(token) = caller {
        if let Some(folder_id) = file.folder_id {
            if has_token_share(token, folder_id) {
                return true;
            }
        }
    }

    false
}
```

`has_user_share` / `has_token_share` はファイルの `folder_id` からフォルダ階層を**祖先方向へ辿り**、いずれかのフォルダに有効な共有レコードがあれば `true` を返す。フォルダ深度は通常浅い（3〜5 段）ため、再帰クエリで許容範囲。

### 4.3 権限レベル

| `permission` | できること |
|-------------|-----------|
| `viewer` | ファイル・フォルダの閲覧・ダウンロード |
| `editor` | `viewer` に加え、ファイルアップロード・削除・フォルダ作成 |

> Phase 1 では `viewer` のみ実装。`editor` は Phase 2 以降。

### 4.4 URL 戦略

**全ファイルの URL は `/v1/drive/files/{id}/content` に統一する。**

| ファイルの種類 | エンドポイント | 認証 |
|--------------|--------------|------|
| 一般ファイル（`project_id = NULL`） | `/v1/drive/files/{id}/content` | 不要 |
| プロジェクトファイル（`project_id` あり） | `/v1/drive/files/{id}/content` | **必要**（セッション or PAT or 共有トークン） |

S3 バックエンドであっても S3 の直 URL はクライアントに渡さない。バックエンドが S3 からストリーム取得してレスポンスする。これにより S3 エンドポイント変更時も DB の URL が陳腐化しない（`storage_key` さえ正しければよい）。

### 4.5 プロジェクトフォルダのライフサイクル

| イベント | 動作 |
|---------|------|
| プロジェクト作成 | `drive_folders` にプロジェクトフォルダを自動作成（`project_id` セット） |
| プロジェクト削除 | CASCADE により `drive_folders` → `drive_files` の順で削除。ストレージオブジェクトも連動削除 |
| ファイルをプロジェクトフォルダへ移動 | `drive_files.project_id` を移動先フォルダの `project_id` に更新 |
| ファイルをプロジェクトフォルダ外へ移動 | `drive_files.project_id` を `NULL` にリセット |

---

## 5. クォータ管理

### 5.1 クォータの 3 層構造

```text
DRIVE_SYSTEM_MAX_QUOTA_MB  ← システム上限（ハードキャップ。設定時はこれを超えて設定不可）
        ↓ 上限として機能
tenants.drive_quota_bytes  ← テナント個別設定（テナントオーナーが変更可）
        ↓ NULL 時のフォールバック
DRIVE_DEFAULT_QUOTA_MB     ← システムデフォルト（テナント未設定時に適用）
```

| 層 | 設定元 | 変更者 | 説明 |
|----|--------|--------|------|
| システム上限 | `DRIVE_SYSTEM_MAX_QUOTA_MB` | サーバー管理者（環境変数） | テナントが設定できる上限の天井。`0` = 天井なし |
| テナント個別 | `tenants.drive_quota_bytes` | テナントオーナー（API） | テナント固有の有効クォータ。`NULL` = デフォルト適用 |
| システムデフォルト | `DRIVE_DEFAULT_QUOTA_MB` | サーバー管理者（環境変数） | テナント個別未設定時のフォールバック。`0` = 無制限 |

**有効クォータの決定ロジック**:

```rust
fn effective_quota(tenant: &Tenant, config: &DriveConfig) -> Option<i64> {
    // None = 無制限
    match tenant.drive_quota_bytes {
        Some(q) => Some(q),                          // テナント個別設定を優先
        None => match config.default_quota_bytes {
            0 => None,                               // デフォルト 0 = 無制限
            q => Some(q),                            // デフォルト値を適用
        },
    }
}
```

**テナントオーナーがクォータを設定する際のバリデーション**:

- `DRIVE_SYSTEM_MAX_QUOTA_MB > 0` の場合: `quota_bytes ≤ system_max` でなければ `400 Bad Request`
- `DRIVE_SYSTEM_MAX_QUOTA_MB = 0`（天井なし）の場合: 制限なく設定可能

### 5.2 使用量の計算

使用量はアップロード時に `drive_files` テーブルを集計して算出する（キャッシュなし、常に正確な値）。

```sql
SELECT COALESCE(SUM(size), 0) FROM drive_files WHERE tenant_id = $1
```

アップロード前に「現在の使用量 + 新ファイルのサイズ ≤ 有効クォータ」を検証し、超過する場合は `413 Content Too Large` を返す。有効クォータが `None`（無制限）の場合は検証をスキップする。

### 5.3 クォータ取得 API

```text
GET /v1/tenants/{tenant_id}/drive/usage
```

レスポンス:

```json
{
  "used_bytes": 524288000,
  "quota_bytes": 10737418240,
  "system_max_bytes": 53687091200,
  "unlimited": false
}
```

| フィールド | 説明 |
|-----------|------|
| `used_bytes` | 現在の使用量（バイト） |
| `quota_bytes` | 有効なクォータ（バイト）。無制限の場合は `null` |
| `system_max_bytes` | システム上限（バイト）。天井なしの場合は `null`。テナントオーナーが設定 UI の上限として使用 |
| `unlimited` | 有効クォータが無制限の場合 `true` |

### 5.4 クォータ設定 API

テナントオーナーがドライブ容量を変更できる。

```text
PATCH /v1/tenants/{tenant_id}/drive/quota
```

リクエスト:

```json
{ "quota_bytes": 10737418240 }
```

- `null` を渡すとシステムデフォルトにリセット
- テナントオーナー権限が必要（既存の `ensure_tenant_owner` を流用）
- `DRIVE_SYSTEM_MAX_QUOTA_MB > 0` の場合、`quota_bytes > system_max` なら `400 Bad Request`

### 5.5 システム上限引き下げ時の挙動

`DRIVE_SYSTEM_MAX_QUOTA_MB` を引き下げた場合、既存テナントの `drive_quota_bytes` は変更しない。
起動時に全テナントをスキャンし、超過テナントを警告ログに出力する:

```text
WARN drive_quota: tenant {tenant_id} ({display_id}) quota {quota_bytes} exceeds system_max {system_max_bytes}
```

将来の Phase 2 で管理者向け監査エンドポイント（例: `GET /v1/admin/drive/quota-violations`）を追加し、超過テナント一覧を UI で確認できるようにする。アップロード時のクォータ検証にはテナントの `drive_quota_bytes`（=現状値）を使うため、上限引き下げが既存テナントのアップロードをブロックすることはない。

---

## 6. ストレージバックエンド

### 6.1 抽象インターフェース（Rust trait）

```rust
#[async_trait]
pub trait StorageBackend: Send + Sync {
    /// ストリーミングアップロード。大ファイルをメモリに全展開しない。
    async fn upload(
        &self,
        key: &str,
        stream: BoxStream<'static, Result<Bytes, StorageError>>,
        content_length: u64,
        mime: &str,
    ) -> Result<(), StorageError>;

    async fn delete(&self, key: &str) -> Result<(), StorageError>;

    /// ストリーミングダウンロード（プロキシ配信用）。
    async fn get_stream(
        &self,
        key: &str,
    ) -> Result<BoxStream<'static, Result<Bytes, StorageError>>, StorageError>;

    async fn public_url(&self, key: &str) -> String;
}
```

> **ストリーミング設計の理由**: `Bytes` を受け取ると最大 `UPLOAD_MAX_SIZE_MB` 分をメモリに全展開してから S3/ローカルへ渡す。100MB ファイルを複数同時アップロードすると GByte 単位のメモリを消費しうる。`BoxStream` を使うことで axum の multipart ストリームをそのままバックエンドへ流し、メモリ使用量をチャンク単位に抑える。S3 の場合は `CreateMultipartUpload` と組み合わせ、ローカルの場合は `tokio::io::copy` でファイルに書き込む。

### 6.2 S3 バックエンド

- クレート: `aws-sdk-s3`
- S3 互換エンドポイントに対応（MinIO / Cloudflare R2 / Backblaze B2）
- バケットのパブリックアクセスポリシーを前提に公開 URL を生成

```env
STORAGE_BACKEND=s3
S3_ENDPOINT=https://s3.amazonaws.com      # MinIO: http://localhost:9000
S3_BUCKET=my-task-drive
S3_REGION=ap-northeast-1
S3_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE
S3_SECRET_ACCESS_KEY=wJalrXUtnFEMI...
S3_PUBLIC_BASE_URL=https://cdn.example.com  # CDN 前段を置く場合（省略時はエンドポイント+バケット名）
S3_FORCE_PATH_STYLE=true                  # MinIO 等で必要。false がデフォルト
```

> **`S3_FORCE_PATH_STYLE`**: AWS S3 は仮想ホスト形式（`bucket.s3.amazonaws.com`）が標準だが、MinIO などのセルフホスト互換では `http://endpoint/bucket/key` 形式（パス形式）が必要。`true` に設定すると `aws-sdk-s3` の `force_path_style` オプションが有効になる。

**アップロードフロー（S3）**:
1. クライアント → `POST /v1/tenants/{id}/drive/files` (multipart)
2. バックエンドが multipart ストリームを受信
3. `aws-sdk-s3` で `PutObject`（< 5MB）または `CreateMultipartUpload`（≥ 5MB）をストリーミングで実行
4. DB に `drive_files` レコードを登録（`storage_key` のみ保持、`url` カラムなし）
5. レスポンスに `/v1/drive/files/{id}/content` を `url` として組み立てて返却

### 6.3 ローカルバックエンド

- 環境変数 `LOCAL_UPLOAD_DIR` で保存先ディレクトリを指定
- バックエンドが `GET /v1/drive/files/{id}/content` でファイルを配信
- 開発環境・セルフホスト向け

```env
STORAGE_BACKEND=local
LOCAL_UPLOAD_DIR=/var/task/uploads
```

**ファイル配信エンドポイント（全バックエンド共通）**:

```text
GET /v1/drive/files/{id}/content
GET /v1/drive/files/{id}/content?token={share_token}
```

- **一般ファイル（`project_id = NULL`）**: 認証不要
- **プロジェクトファイル（`project_id` あり）**: 以下いずれかが必要
  - セッション or PAT 認証（そのプロジェクトに入れる人またはテナントオーナー）
  - 有効な `share_token`（`?token=` クエリパラメータ）
  - いずれも満たさない場合 → `403 Forbidden`
- `Content-Type` を `mime_type` から設定
- `Content-Disposition: attachment` を常に設定し、ブラウザ上で同一オリジンのコンテンツとして実行させない
- API 共通ミドルウェアが `X-Content-Type-Options: nosniff` を全レスポンスに設定し、宣言した `Content-Type` 以外への MIME sniffing を禁止する
- ストレージバックエンドの `get_stream()` でストリーミング配信（メモリに全展開しない）

---

## 7. PAT スコープ

### 7.1 既存スコープとの関係

現在定義されているスコープ:

| スコープ | 説明 |
|---------|------|
| `read:project` | プロジェクトの読み取り |
| `write:project` | プロジェクトの作成・更新・削除 |
| `admin:tenant` | テナント管理全般（他スコープを暗黙的に包含） |

`admin:tenant` を持つ PAT はドライブ操作を含むすべての操作が可能（既存の `ScopeList::has_scope` が `AdminTenant` を最上位として扱う）。

### 7.2 Drive 用新スコープ

| スコープ名 | 説明 |
|-----------|------|
| `read:drive` | ドライブのファイル・フォルダ一覧取得、ダウンロード、使用量確認 |
| `write:drive` | ファイルアップロード・削除・移動、フォルダ作成・削除・移動、共有の作成・取り消し |

### 7.3 エンドポイント別必要スコープ一覧

| メソッド | パス | 必要スコープ | 備考 |
|---------|------|------------|------|
| `GET` | `/v1/tenants/{id}/drive/files` | `read:drive` | |
| `POST` | `/v1/tenants/{id}/drive/files` | `write:drive` | クォータ検証あり |
| `GET` | `/v1/tenants/{id}/drive/files/{id}` | `read:drive` | |
| `PATCH` | `/v1/tenants/{id}/drive/files/{id}` | `write:drive` | |
| `DELETE` | `/v1/tenants/{id}/drive/files/{id}` | `write:drive` | |
| `GET` | `/v1/drive/files/{id}/content` | `read:drive` | プロジェクトファイルのみ認証必要 |
| `GET` | `/v1/drive/files/{id}/content?token=` | スコープ不要 | 公開リンクトークンで代替 |
| `GET` | `/v1/tenants/{id}/drive/usage` | `read:drive` | |
| `PATCH` | `/v1/tenants/{id}/drive/quota` | `admin:tenant` | テナントオーナー限定 |
| `GET` | `/v1/tenants/{id}/drive/folders` | `read:drive` | |
| `POST` | `/v1/tenants/{id}/drive/folders` | `write:drive` | |
| `PATCH` | `/v1/tenants/{id}/drive/folders/{id}` | `write:drive` | |
| `DELETE` | `/v1/tenants/{id}/drive/folders/{id}` | `write:drive` | |
| `GET` | `/v1/tenants/{id}/drive/folders/{id}/shares` | `read:drive` | |
| `POST` | `/v1/tenants/{id}/drive/folders/{id}/shares` | `write:drive` | |
| `DELETE` | `/v1/tenants/{id}/drive/folders/{id}/shares/{share_id}` | `write:drive` | |
| `GET` | `/v1/drive/share/{token}` | スコープ不要 | 公開リンク（認証不要） |
| `GET` | `/v1/drive/share/{token}/files` | スコープ不要 | 公開リンク（認証不要） |

### 7.4 実装上の注意

`entities/scopes.rs` の `Scope` enum に以下を追加する:

```rust
#[serde(rename = "read:drive")]
ReadDrive,
#[serde(rename = "write:drive")]
WriteDrive,
```

`write:drive` は `read:drive` を暗黙的に包含する（`write` を持つなら `read` も可能）。
`has_scope` の実装でこの包含関係を反映する:

```rust
pub fn has_scope(&self, scope: Scope) -> bool {
    self.0.contains(&scope)
        || self.0.contains(&Scope::AdminTenant)
        || (scope == Scope::ReadDrive && self.0.contains(&Scope::WriteDrive))
}
```

---

## 8. API 設計

全エンドポイントはセッション認証 or PAT 認証が必須（既存の `auth` ミドルウェアを流用）。

### 8.1 ファイル API

| メソッド | パス | 説明 |
|---------|------|------|
| `GET` | `/v1/tenants/{tenant_id}/drive/files` | ファイル一覧 |
| `POST` | `/v1/tenants/{tenant_id}/drive/files` | ファイルアップロード |
| `GET` | `/v1/tenants/{tenant_id}/drive/files/{id}` | ファイルメタデータ取得 |
| `PATCH` | `/v1/tenants/{tenant_id}/drive/files/{id}` | ファイル更新（名前・フォルダ移動） |
| `DELETE` | `/v1/tenants/{tenant_id}/drive/files/{id}` | ファイル削除 |
| `GET` | `/v1/drive/files/{id}/content` | ファイル内容配信（ローカル・S3 プロキシ） |
| `GET` | `/v1/tenants/{tenant_id}/drive/usage` | 使用量・クォータ取得 |
| `PATCH` | `/v1/tenants/{tenant_id}/drive/quota` | クォータ設定（テナントオーナーのみ） |
| `GET` | `/v1/drive/files/{id}/content?token={token}` | 公開リンクトークンによるファイル配信 |

#### GET `/v1/tenants/{tenant_id}/drive/files`

クエリパラメータ:

| パラメータ | 型 | デフォルト | 説明 |
|-----------|-----|----------|------|
| `folder_id` | UUID? | - | フォルダ絞り込み（省略時はルート） |
| `limit` | u32 | 50 | 取得件数（最大 200） |
| `offset` | u32 | 0 | オフセット |

レスポンス:

```json
{
  "files": [
    {
      "id": "...",
      "name": "screenshot.png",
      "size": 204800,
      "mime_type": "image/png",
      "url": "/v1/drive/files/xxxxxxxx-.../content",
      "folder_id": null,
      "created_at": "2026-05-26T12:00:00Z"
    }
  ],
  "total": 42
}
```

#### POST `/v1/tenants/{tenant_id}/drive/files`

リクエスト: `multipart/form-data`

| フィールド | 必須 | 説明 |
|-----------|------|------|
| `file` | ✓ | ファイルバイナリ |
| `name` | - | 表示名（省略時は元ファイル名） |
| `folder_id` | - | アップロード先フォルダ UUID |

レスポンス: 作成された `DriveFile` オブジェクト (201 Created)

制限:
- 最大ファイルサイズ: 環境変数 `UPLOAD_MAX_SIZE_MB` で設定（デフォルト 100MB）
- 許可 MIME タイプ: 全種類

### 8.2 フォルダ API

| メソッド | パス | 説明 |
|---------|------|------|
| `GET` | `/v1/tenants/{tenant_id}/drive/folders` | フォルダ一覧 |
| `POST` | `/v1/tenants/{tenant_id}/drive/folders` | フォルダ作成 |
| `PATCH` | `/v1/tenants/{tenant_id}/drive/folders/{id}` | フォルダ更新（名前変更・移動） |
| `DELETE` | `/v1/tenants/{tenant_id}/drive/folders/{id}` | フォルダ削除 |

フォルダ削除時の挙動:
- フォルダ内ファイルが存在する場合: **`409 Conflict`** を返し削除しない（強制削除は Phase 2 以降）

### 8.3 フォルダ共有 API

| メソッド | パス | 説明 |
|---------|------|------|
| `GET` | `/v1/tenants/{tenant_id}/drive/folders/{id}/shares` | 共有一覧 |
| `POST` | `/v1/tenants/{tenant_id}/drive/folders/{id}/shares` | 共有作成 |
| `DELETE` | `/v1/tenants/{tenant_id}/drive/folders/{id}/shares/{share_id}` | 共有取り消し |
| `GET` | `/v1/drive/share/{token}` | 公開リンクでフォルダにアクセス（認証不要） |
| `GET` | `/v1/drive/share/{token}/files` | 公開リンク経由でファイル一覧取得 |

#### POST `/v1/tenants/{tenant_id}/drive/folders/{id}/shares`

リクエスト（ユーザー指定共有）:

```json
{
  "type": "user",
  "user_id": "xxxxxxxx-...",
  "permission": "viewer",
  "expires_at": null
}
```

リクエスト（公開リンク共有）:

```json
{
  "type": "public_link",
  "permission": "viewer",
  "expires_at": "2026-12-31T23:59:59Z"
}
```

レスポンス（公開リンク共有の場合）:

```json
{
  "id": "...",
  "share_token": "abc123xyz",
  "permission": "viewer",
  "expires_at": "2026-12-31T23:59:59Z"
}
```

- `share_url` は返さない。フロントエンドが `window.location.origin + "/drive/share/" + share_token` で組み立てる
- `share_token` は URL-safe な 32 文字ランダム文字列
- フォルダ作成者またはテナントオーナーのみ共有操作可
- Phase 1 で `permission: "editor"` を指定した場合 → `422 Unprocessable Entity`（`editor` は Phase 2 以降）

#### GET `/v1/drive/share/{token}`

- 認証不要
- フォルダメタデータ（名前、作成者名、ファイル数）を返す
- 有効期限切れの場合は `410 Gone`

---

## 9. フロントエンド UI 設計

### 9.1 ページ構成

```text
/tenants/{tenant_id}/drive       # ドライブトップ（ルートフォルダ）
/tenants/{tenant_id}/drive/folders/{folder_id}  # フォルダ内
```

### 9.2 レイアウト

```text
┌─────────────────────────────────────────────────────┐
│ Breadcrumb: ドライブ > フォルダA > サブフォルダB      │
├────────────────┬────────────────────────────────────┤
│                │  ┌──────────────────────────────┐  │
│ [+ 新しい      │  │ 🔍 ファイル検索               │  │
│   フォルダ]    │  └──────────────────────────────┘  │
│                │                                    │
│ ▼ ドライブ     │  [▲ アップロード]  [リスト/グリッド] │
│   フォルダA    │                                    │
│   フォルダB    │  📁 フォルダA    📁 フォルダB       │
│               │  📄 report.pdf  🖼 image.png        │
└────────────────┴────────────────────────────────────┘
```

### 9.3 コンポーネント構成

| コンポーネント | ファイル | 説明 |
|--------------|---------|------|
| `DrivePage` | `pages/tenants/[id]/drive/index.vue` | ドライブ全体 |
| `DriveSidebar` | `components/drive/DriveSidebar.vue` | フォルダツリー |
| `DriveFileGrid` | `components/drive/DriveFileGrid.vue` | ファイル一覧（グリッド） |
| `DriveFileList` | `components/drive/DriveFileList.vue` | ファイル一覧（リスト） |
| `DriveUploadButton` | `components/drive/DriveUploadButton.vue` | ドラッグ&ドロップ対応 |
| `DriveFileCard` | `components/drive/DriveFileCard.vue` | ファイルカード（グリッド用） |
| `DriveFileDetail` | `components/drive/DriveFileDetail.vue` | 詳細パネル（サイドシート） |

### 9.4 主要インタラクション

- **アップロード**: ボタンクリック or エリアへドラッグ&ドロップ → プログレスバー表示
- **フォルダ作成**: サイドバーの「+ 新しいフォルダ」ボタン → インライン入力
- **ファイル詳細**: ファイルカードをクリック → 右サイドシートで詳細表示・URL コピー
- **削除**: 右クリックメニュー or 詳細パネルの削除ボタン → 確認ダイアログ

---

## 10. セキュリティ

| 脅威 | 対策 |
|------|------|
| 他テナントのファイルへのアクセス | 全エンドポイントで `tenant_id` の所有権チェック |
| プロジェクト外ユーザーによるファイルアクセス | `drive_files.project_id` で判定し、非メンバーは `403` |
| S3 の直接 URL によるプロジェクトファイルの漏洩 | プロジェクトファイルの S3 URL はクライアントに渡さない。バックエンドプロキシ経由のみ |
| 任意ファイル上書き | ストレージキーは UUID v4 で生成（衝突なし） |
| 超大型ファイルによる DoS | `axum` の `DefaultBodyLimit` を `UPLOAD_MAX_SIZE_MB` から動的設定 |
| パストラバーサル（ローカル） | ストレージキーは UUID のみ使用。元ファイル名はメタデータのみ |
| 共有トークンの総当たり | トークンは 32 文字 URL-safe ランダム（エントロピー ≥ 192bit）。レートリミット適用 |
| 期限切れ共有トークンの悪用 | `expires_at` を毎回チェックし、期限切れは `410 Gone` |
| 共有経由の過剰アクセス | `viewer` 共有では削除・アップロード API を `403` でブロック |

---

## 11. 設定まとめ

`apps/backend/.env` に追加する環境変数:

```env
# ストレージバックエンド（必須）
STORAGE_BACKEND=local                 # "local" または "s3"

# アップロード・クォータ設定（共通）
UPLOAD_MAX_SIZE_MB=100                # 1ファイルあたりの上限 MB（デフォルト 100）
DRIVE_SYSTEM_MAX_QUOTA_MB=51200       # テナントが設定できる容量の上限 MB（デフォルト 50GB）。0 = 天井なし
DRIVE_DEFAULT_QUOTA_MB=10240          # テナントデフォルト容量 MB（デフォルト 10GB）。0 = 無制限

# S3 用（STORAGE_BACKEND=s3 の場合）
S3_ENDPOINT=https://s3.amazonaws.com
S3_BUCKET=
S3_REGION=ap-northeast-1
S3_ACCESS_KEY_ID=
S3_SECRET_ACCESS_KEY=
S3_PUBLIC_BASE_URL=                   # CDN を使う場合（省略時はエンドポイント+バケット名）
S3_FORCE_PATH_STYLE=false             # MinIO 等では true に設定

# ローカル用（STORAGE_BACKEND=local の場合）
LOCAL_UPLOAD_DIR=./uploads
```

---

## 12. 実装計画

### ブランチ名
```text
feat/drive
```

### タスク分解

**実装順序: バックエンド完全完了後にフロントエンドへ移行する。**

#### Phase A — バックエンド

| # | 内容 |
|---|------|
| 1 | `StorageBackend` trait + Local 実装 |
| 2 | S3 実装（`aws-sdk-s3`、`force_path_style` 対応） |
| 3 | `Scope` enum に `ReadDrive` / `WriteDrive` 追加、`has_scope` の包含関係更新 |
| 4 | `tenants` に `drive_quota_bytes` カラム追加 |
| 5 | `drive_folders` エンティティ + API（`project_id` 含む） |
| 6 | `drive_files` エンティティ + API（`project_id` 含む、アップロード時クォータ検証） |
| 7 | ファイル配信エンドポイント（アクセス制御付き、S3 プロキシ対応） |
| 8 | クォータ取得・設定 API（`GET/PATCH /drive/usage`, `/drive/quota`） |
| 9 | プロジェクト作成フック — ドライブフォルダ自動作成 |
| 10 | `drive_folder_shares` エンティティ + 共有 API（ユーザー指定・公開リンク） |
| 11 | 公開リンクトークン経由のファイル配信・フォルダ閲覧エンドポイント |
| 12 | Bruno で全エンドポイントの動作確認（Phase A 完了条件） |

#### Phase B — フロントエンド（Phase A 完了後に着手）

| # | 内容 |
|---|------|
| 13 | `pnpm openapi` でクライアント再生成 |
| 14 | `DrivePage` + `DriveSidebar`（プロジェクトフォルダを識別表示） |
| 15 | `DriveFileGrid` + `DriveUploadButton`（ドラッグ&ドロップ） |
| 16 | `DriveFileDetail` サイドシート |
| 17 | クォータ使用量バー（ドライブ画面下部に表示） |
| 18 | フォルダ共有 UI（共有ダイアログ・`share_token` からリンク生成・コピー） |

---

## 13. 決定事項ログ

| 項目 | 決定内容 | 決定日 |
|------|---------|--------|
| ファイルサイズ上限 | `UPLOAD_MAX_SIZE_MB` 環境変数で設定変更可（デフォルト 100MB） | 2026-05-26 |
| 一般ファイルの配信認証 | 認証不要。リンクを知っていれば誰でもアクセス可 | 2026-05-26 |
| プロジェクトファイルのアクセス制御 | プロジェクトメンバーまたはテナントオーナーのみ閲覧可 | 2026-05-26 |
| プロジェクトファイルのアクセス制御（改訂） | テナントに所属していることを先に確認したうえで、プロジェクトの公開規則で判定する | 2026-08-18 |
| プロジェクトファイルの URL 戦略 | バックエンド経由でプロキシ（S3 URL は直接渡さない） | 2026-05-26 |
| S3 バケット公開設定 | パブリックバケット（プロジェクトファイルのキーは露出しない） | 2026-05-26 |
| フォルダ削除の挙動 | ファイルが存在する場合は `409 Conflict`（強制削除は Phase 2） | 2026-05-26 |
| MIME タイプ制限 | Phase 1 は全種類許可 | 2026-05-26 |
| S3 ForcePathStyle | `S3_FORCE_PATH_STYLE` 環境変数で設定可（MinIO 等向け） | 2026-05-26 |
| プロジェクトフォルダ自動作成 | プロジェクト作成時に対応するドライブフォルダを自動作成 | 2026-05-26 |
| テナント別ドライブ容量 | `tenants.drive_quota_bytes` で個別設定。`NULL` 時は `DRIVE_DEFAULT_QUOTA_MB` 適用（`0` = 無制限） | 2026-05-26 |
| システム上限（ハードキャップ） | `DRIVE_SYSTEM_MAX_QUOTA_MB` で設定。テナントオーナーはこの値を超えて設定不可。`0` = 天井なし | 2026-05-26 |
| クォータ超過時のレスポンス | `413 Content Too Large` | 2026-05-26 |
| システム上限違反時のレスポンス | `400 Bad Request` | 2026-05-26 |
| フォルダ共有 | ユーザー指定共有 + 公開リンク共有の 2 種類。配下サブフォルダ・ファイルに再帰継承 | 2026-05-26 |
| 共有権限 Phase 1 | `viewer`（閲覧のみ）のみ実装。`editor` は Phase 2 | 2026-05-26 |
| 公開リンクトークン | 32 文字 URL-safe ランダム。有効期限設定可（NULL = 無期限）| 2026-05-26 |
| PAT スコープ | `read:drive`（閲覧）・`write:drive`（書き込み）を新設。`write:drive` は `read:drive` を包含。`admin:tenant` は全 Drive 操作を包含 | 2026-05-26 |
| ストリーミング | `StorageBackend` trait は `BoxStream` を使いメモリ全展開を回避 | 2026-05-26 |
| URL 管理 | `drive_files.url` カラム廃止。全ファイルを `/v1/drive/files/{id}/content` で統一配信 | 2026-05-26 |
| 不変条件保護 | `project_id IS NULL OR folder_id IS NOT NULL` を DB CHECK 制約 + アプリバリデーション両方で強制 | 2026-05-26 |
| editor 権限 Phase 1 | editor を指定した場合は `422 Unprocessable Entity`。実装は Phase 2 | 2026-05-26 |
| share_url | バックエンドは `share_token` のみ返す。フロントが `window.location.origin` で URL を組み立てる | 2026-05-26 |
| 上限引き下げ時の挙動 | 既存テナントの値は変更せず起動時に警告ログ出力。Phase 2 で管理者向け監査エンドポイントを追加 | 2026-05-26 |
| 階層と `project_id` の整合 | フォルダの `project_id` は階層のルートから継承し、移動時は配下（フォルダ・ファイル）まで揃える。既に食い違っている行は backfill マイグレーション（`m20260904000000_drive_project_id_backfill`）でプロジェクトルートから辿って直す。一般ツリーの配下に残った `project_id` は触らない（階層より厳しい判定になるだけで、NULL へ落とすと非メンバーへ開く） | 2026-09-04 |
| backfill の中断条件 | 配下に**別プロジェクト**の `project_id` を持つ行があるツリーは 1 行も書き換えず、マイグレーションを失敗させる。継承で上書きするとそのプロジェクトのファイルが別プロジェクトのメンバーへ公開され、元のメンバーはアクセスを失う（修正前はプロジェクトルートの移動もできたため、この状態が既存データにありうる）。該当フォルダを人が直してから流し直す | 2026-09-04 |
| 階層変更の直列化 | フォルダの作成・移動、ファイルの作成・移動は、テナント単位のアドバイザリロック（`pg_advisory_xact_lock`）で直列化する。ACL と親の `project_id` はロック取得後に同一トランザクションで読み、挿入・移動・子孫の同期まで同じトランザクションで終える。ロックの外で読むと、移動中のフォルダへ子を作ったときに移動前の `project_id` を継承した行が同期の後から挿入され、ACL 漏れが再発する | 2026-09-04 |
