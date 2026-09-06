---
title: Drive 機能仕様
description: Misskey ライクなドライブ機能の設計仕様書
icon: lucide:hard-drive
---

# Drive 機能仕様書

> ステータス: **バックエンド実装済み／フロントエンド未実装（既知の差分あり）**
> 作成日: 2026-05-26
> 最終更新: 2026-09-03（現在の実装・認可方針・既知の差分を反映）

---

## 1. 概要

タスク管理 SaaS に Misskey ライクなドライブ機能を追加する。
ドライブはテナント単位のファイル管理スペースであり、アップロードしたファイルをフォルダで整理し、タスクへの添付や共有 URL 発行に利用できる。

ストレージバックエンドは **S3 互換（AWS S3 / MinIO）** と **ローカルディスク** の 2 種類をサポートし、環境変数で切り替える。

---

## 2. スコープ

### 現在の実装範囲

| 機能 | 状態 | 内容 |
|------|------|------|
| ファイル CRUD | 実装済み | multipart/form-data アップロード、一覧、メタデータ、名前・配置・本文更新、削除 |
| ストレージ | 実装済み | S3 互換またはローカルディスクを環境変数で切り替え |
| ファイル配信 | 実装済み | 全バックエンドを `/v1/drive/files/{id}/content` からプロキシ配信 |
| フォルダ CRUD | 実装済み | 階層フォルダの作成・一覧・更新・削除 |
| フォルダ共有 | 一部差分あり | ユーザー指定共有、公開リンク共有、共有トークンによる本文取得 |
| クォータ | 実装済み | 使用量取得、テナント個別クォータ設定、アップロード・本文更新時の検証 |
| タスク添付 | 実装済み | Drive ファイルとタスクの紐付け・解除 |
| フロントエンド UI | 未実装 | OpenAPI クライアント用定義と PAT スコープ表示のみ存在 |

実装済みとした項目にも、仕様を完全には満たしていない箇所がある。詳細は
[§12 現在の実装との差分・既知の問題](#12-現在の実装との差分既知の問題)を参照。

### 今後の拡張

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
    pub updated_at: DateTimeUtc,
}
```

| カラム | 型 | 制約 | 説明 |
|--------|-----|------|------|
| `id` | UUID | PK | |
| `name` | VARCHAR | NOT NULL | ファイル表示名 |
| `size` | BIGINT | NOT NULL | バイト数 |
| `mime_type` | VARCHAR | NOT NULL | |
| `storage_type` | VARCHAR(16) | NOT NULL | Rust 側では enum。`s3` または `local` |
| `storage_key` | VARCHAR | NOT NULL | ストレージ固有キー（UUID v4）|
| `tenant_id` | UUID | NOT NULL, FK→tenants CASCADE | |
| `project_id` | UUID | NULLABLE | フォルダの `project_id` を非正規化コピー。アクセス制御の高速判定に使用 |
| `uploader_id` | UUID | NOT NULL, FK→users | |
| `folder_id` | UUID | NULLABLE, FK→drive_folders SET NULL | |
| `created_at` | TIMESTAMPTZ | NOT NULL DEFAULT now() | |
| `updated_at` | TIMESTAMPTZ | NOT NULL DEFAULT now() | |

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
| `permission` | VARCHAR(16) | NOT NULL | Rust 側では enum。現行値は `viewer` のみ |
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
| `project_id = NULL`（テナント一般ファイル） | ① テナント所属者 ② テナントオーナー ③ フォルダ共有権限を持つユーザー/トークン |
| `project_id = <id>`（プロジェクトファイル） | ① そのプロジェクトに入れる人 ② テナントオーナー ③ フォルダ共有権限を持つユーザー/トークン |

①は「ファイルの `tenant_id` に所属していること」を先に確認したうえで、プロジェクトの公開規則で判定する。
ファイル ID だけで引ける配信経路があるため、プロジェクト所属だけを見るとテナント境界を越えられる。
判定の詳細は [テナント / プロジェクト認可](../../apps/backend/docs/tenant-project-authz.md) を参照。

③のフォルダ共有は所属とは独立した明示的な付与なので、テナントから外しても失効しない。

### 4.2 アクセス判定ロジック

```rust
// アクセス可否チェックの擬似コード
fn can_access_file(file: &DriveFile, caller: &Caller) -> bool {
    // 共有トークンはテナント所属と独立して判定する
    if let Caller::ShareToken(token) = caller {
        return file.folder_id
            .is_some_and(|folder_id| has_token_share(token, folder_id));
    }

    // 認証済みユーザーの場合
    if let Caller::User(user) = caller {
        if !has_scope(user, "read:drive") {
            return false;
        }

        // テナント一般ファイルはテナント所属者またはオーナーに許可
        if file.project_id.is_none() {
            return can_access_tenant(user.id, file.tenant_id)
                || file.folder_id.is_some_and(|id| has_user_share(user.id, id));
        }

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

    false
}
```

`has_user_share` / `has_token_share` はファイルの `folder_id` からフォルダ階層を**祖先方向へ辿り**、いずれかのフォルダに有効な共有レコードがあれば `true` を返す。フォルダ深度は通常浅い（3〜5 段）ため、再帰クエリで許容範囲。

### 4.3 権限レベル

| `permission` | できること |
|-------------|-----------|
| `viewer` | ファイル・フォルダの閲覧・ダウンロード |
| `editor` | `viewer` に加え、ファイルアップロード・削除・フォルダ作成 |

> 現在は `viewer` のみ実装。`editor` は将来対応。

### 4.4 URL 戦略

**全ファイルの URL は `/v1/drive/files/{id}/content` に統一する。**

| ファイルの種類 | エンドポイント | 認証 |
|--------------|--------------|------|
| テナント一般ファイル（`project_id = NULL`） | `/v1/drive/files/{id}/content` | **必要**（セッション or PAT or 共有トークン） |
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
    // None = 無制限。システム上限があれば常に最後に適用する。
    let requested = tenant.drive_quota_bytes
        .unwrap_or(config.default_quota_bytes);
    let requested = (requested != 0).then_some(requested);
    let system_max = (config.system_max_quota_bytes != 0)
        .then_some(config.system_max_quota_bytes);

    match (requested, system_max) {
        (Some(q), Some(max)) => Some(q.min(max)),
        (None, Some(max)) => Some(max),
        (Some(q), None) => Some(q),
        (None, None) => None,
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

`DRIVE_SYSTEM_MAX_QUOTA_MB` を引き下げた場合、既存テナントの `drive_quota_bytes` 自体は変更しない。
起動時に全テナントをスキャンし、超過テナントを警告ログに出力する:

```text
WARN drive_quota: tenant {tenant_id} ({display_id}) quota {quota_bytes} exceeds system_max {system_max_bytes}
```

将来の拡張で管理者向け監査エンドポイント（例: `GET /v1/admin/drive/quota-violations`）を追加し、超過テナント一覧を UI で確認できるようにする。実行時の有効クォータにはシステム上限が常に適用されるため、使用量が新しい上限以上のテナントでは、削除などで使用量を減らすまで新規アップロードと容量を増やす本文更新が `413 Content Too Large` になる。

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
}
```

> **ストリーミング設計の理由**: `Bytes` を受け取ると最大 `UPLOAD_MAX_SIZE_MB` 分をメモリに全展開してから S3/ローカルへ渡す。100MB ファイルを複数同時アップロードすると GByte 単位のメモリを消費しうる。`BoxStream` を使うことで axum の multipart ストリームをそのままバックエンドへ流し、メモリ使用量をチャンク単位に抑える。S3 の場合は `CreateMultipartUpload` と組み合わせ、ローカルの場合は `tokio::io::copy` でファイルに書き込む。

### 6.2 S3 バックエンド

- クレート: `object_store`（`AmazonS3Builder`）
- S3 互換エンドポイントに対応（MinIO / Cloudflare R2 / Backblaze B2）
- バケットは非公開でよい。ファイルはバックエンドからプロキシ配信する

```env
STORAGE_BACKEND=s3
S3_ENDPOINT=https://s3.amazonaws.com      # MinIO: http://localhost:9000
S3_BUCKET=my-task-drive
S3_REGION=ap-northeast-1
S3_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE
S3_SECRET_ACCESS_KEY=wJalrXUtnFEMI...
S3_FORCE_PATH_STYLE=true                  # MinIO 等で必要。false がデフォルト
```

> **`S3_FORCE_PATH_STYLE`**: AWS S3 は仮想ホスト形式（`bucket.s3.amazonaws.com`）が標準だが、MinIO などのセルフホスト互換では `http://endpoint/bucket/key` 形式（パス形式）が必要。`true` に設定すると `AmazonS3Builder` の仮想ホスト形式を無効にする。

**アップロードフロー（S3）**:
1. クライアント → `POST /v1/tenants/{id}/drive/files` (multipart)
2. バックエンドが multipart ストリームを受信
3. `object_store` で単一 PUT または multipart upload を実行
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

- **テナント一般ファイル（`project_id = NULL`）**: テナント所属者・オーナー、または有効な `share_token` が必要
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
| `GET` | `/v1/drive/files/{id}/content` | `read:drive` | 共有トークン利用時はスコープ不要 |
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

テナント配下の管理 API はセッション認証または PAT 認証が必須。`GET /v1/drive/files/{id}/content`
は認証任意だが、実際の取得にはテナント／プロジェクト権限または共有トークンが必要。
`GET /v1/drive/share/{token}` とその `/files` は共有トークン自体を認証情報として扱う。

### 8.1 ファイル API

| メソッド | パス | 説明 |
|---------|------|------|
| `GET` | `/v1/tenants/{tenant_id}/drive/files` | ファイル一覧 |
| `POST` | `/v1/tenants/{tenant_id}/drive/files` | ファイルアップロード |
| `GET` | `/v1/tenants/{tenant_id}/drive/files/{id}` | ファイルメタデータ取得 |
| `PATCH` | `/v1/tenants/{tenant_id}/drive/files/{id}` | ファイル更新（名前・フォルダ移動） |
| `PUT` | `/v1/tenants/{tenant_id}/drive/files/{id}/content` | 編集可能なテキストファイルの本文更新 |
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
      "created_at": "2026-05-26T12:00:00Z",
      "updated_at": "2026-05-26T12:00:00Z"
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

現在のストリーミング実装では、`name` と `folder_id` は `file` パートより前に送る必要がある。
`file` を処理した時点でレスポンスを返すため、それ以降のパートは解釈されない。この順序制約は
OpenAPI だけでは表現できないため、クライアント実装でも明示的に順序を固定する。

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
- 直下にファイルまたは子フォルダが存在する場合: **`409 Conflict`** を返し削除しない（強制削除は将来対応）

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
- 現在 `permission: "editor"` を指定した場合 → `422 Unprocessable Entity`（`editor` は将来対応）

#### GET `/v1/drive/share/{token}`

- 認証不要
- フォルダメタデータ（名前、作成者名、ファイル数）を返す
- 有効期限切れの場合は `410 Gone`

### 8.4 タスク添付 API

Drive ファイルは既存タスクへ添付できる。添付は中間レコードの作成・削除であり、解除しても
Drive ファイル本体は削除しない。

| メソッド | パス | 必要スコープ | 説明 |
|---------|------|------------|------|
| `GET` | `/v1/tenants/{tenant_id}/projects/{project_id}/tasks/{id}/attachments` | `read:task` | 添付一覧 |
| `POST` | `/v1/tenants/{tenant_id}/projects/{project_id}/tasks/{id}/attachments` | `write:task` | `{ "drive_file_id": "uuid" }` を添付 |
| `DELETE` | `/v1/tenants/{tenant_id}/projects/{project_id}/tasks/{id}/attachments/{attachment_id}` | `write:task` | 添付を解除 |

添付できるのは同じテナントの一般ファイル、または対象タスクと同じプロジェクトのファイル。
別テナント・別プロジェクトのファイルは `403 Forbidden` とする。同じファイルの二重添付は
`409 Conflict` とする。添付解除は添付を作成したユーザーまたはテナントオーナーに限る。

---

## 9. フロントエンド UI 設計

### 9.1 ページ構成

```text
/{tenant}/drive              # ドライブトップ（ルートフォルダ）
/{tenant}/drive/{folder_id}  # フォルダ内
```

> 現在はページ・コンポーネントとも未実装。上記は
> [`docs/frontend/url-spec.md`](../frontend/url-spec.md) に合わせた予定 URL。

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
| `DrivePage` | `src/pages/@tenant/drive/+Page.vue` | ドライブ全体 |
| `DriveFolderPage` | `src/pages/@tenant/drive/@folderId/+Page.vue` | フォルダ内 |
| `DriveSidebar` | `src/components/drive/DriveSidebar.vue` | フォルダツリー |
| `DriveFileGrid` | `src/components/drive/DriveFileGrid.vue` | ファイル一覧（グリッド） |
| `DriveFileList` | `src/components/drive/DriveFileList.vue` | ファイル一覧（リスト） |
| `DriveUploadButton` | `src/components/drive/DriveUploadButton.vue` | ドラッグ&ドロップ対応 |
| `DriveFileCard` | `src/components/drive/DriveFileCard.vue` | ファイルカード（グリッド用） |
| `DriveFileDetail` | `src/components/drive/DriveFileDetail.vue` | 詳細パネル（サイドシート） |

### 9.4 主要インタラクション

- **アップロード**: ボタンクリック or エリアへドラッグ&ドロップ → プログレスバー表示
- **フォルダ作成**: サイドバーの「+ 新しいフォルダ」ボタン → インライン入力
- **ファイル詳細**: ファイルカードをクリック → 右サイドシートで詳細表示・URL コピー
- **削除**: 右クリックメニュー or 詳細パネルの削除ボタン → 確認ダイアログ

---

## 10. セキュリティ

| 脅威 | 対策 |
|------|------|
| 他テナントのファイルへのアクセス | ファイル自身の `tenant_id` に対する所属、または有効な共有トークンを確認 |
| プロジェクト外ユーザーによるファイルアクセス | `drive_files.project_id` で判定し、非メンバーは `403` |
| S3 の直接 URL によるプロジェクトファイルの漏洩 | プロジェクトファイルの S3 URL はクライアントに渡さない。バックエンドプロキシ経由のみ |
| 任意ファイル上書き | ストレージキーは UUID v4 で生成（衝突なし） |
| 超大型ファイルによる DoS | `RequestBodyLimitLayer` とストリーム中の実測値を `UPLOAD_MAX_SIZE_MB` から設定 |
| パストラバーサル（ローカル） | ストレージキーは UUID のみ使用。元ファイル名はメタデータのみ |
| 共有トークンの総当たり | トークンは 32 文字 URL-safe ランダム（エントロピー 192bit）。専用レートリミットは未実装 |
| 期限切れ共有トークンの悪用 | `expires_at` を毎回確認。公開フォルダ API は `410`、ファイル本文 API は `403` |
| 共有経由の過剰アクセス | `viewer` 共有では削除・アップロード API を `403` でブロック |

---

## 11. 設定まとめ

`apps/backend/.env` に追加する環境変数:

```env
# ストレージバックエンド（未指定時は local）
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
S3_FORCE_PATH_STYLE=false             # MinIO 等では true に設定

# ローカル用（STORAGE_BACKEND=local の場合）
LOCAL_UPLOAD_DIR=./uploads
```

---

## 12. 現在の実装との差分・既知の問題

2026-09-03 時点で、次の差分を確認している。ここに記載した項目は期待仕様ではなく、
修正対象として追跡するための現状説明である。

| 優先度 | 項目 | 現状と影響 |
|--------|------|------------|
| 高 | 公開共有ルートの二重 prefix | `public_routes` を `/v1/drive` に nest している一方、共有ハンドラーも `/v1/drive/share/...` を宣言している。実際の OpenAPI は `/v1/drive/v1/drive/share/...` となり、仕様の URL でフォルダ情報・一覧を取得できない |
| 高 | プロジェクトフォルダの継承・ACL | 手動作成した子フォルダは親がプロジェクトフォルダでも `project_id = NULL` になる。フォルダ／ファイル移動時も移動先 ACL と配下の `project_id` を同期しておらず、プロジェクト境界を跨ぐ階層を作れる。空のプロジェクトルートフォルダも通常の削除 API から削除できる |
| 高 | S3 アップロードの全量バッファ | multipart からストレージへ `content_length = 0` を渡すため、S3 実装が常に単一 PUT 分岐へ入り、ファイル全体をメモリに保持する。§6.1 のストリーミング要件を満たしていない |
| 高 | ストレージ削除の非同期整合性 | 個別削除は DB 削除後のストレージエラーを無視し、プロジェクト／テナントの CASCADE 削除ではストレージ削除自体を行わない。孤児オブジェクトを回収する仕組みが必要 |
| 中 | ユーザー指定共有の一覧導線 | 本文取得ではテナント外の共有受信者も許可できるが、ファイル・フォルダ一覧とメタデータ API が先にテナント所属を要求するため、共有受信者が対象を発見できない |
| 中 | 公開共有一覧の再帰性 | `/share/{token}/files` は共有フォルダ直下のファイルだけを返し、子フォルダとそのファイルを列挙しない。本文取得時のトークン判定だけは祖先方向へ継承する |
| 中 | multipart の実効上限 | `UPLOAD_MAX_SIZE_MB` と同じ値をリクエスト body 全体へ適用するため、multipart のヘッダー・境界分だけ、受理できるファイル本体は設定値より小さい |
| 中 | ストレージ種別切り替え | DB に `storage_type` を保存するが、取得・削除は起動中の単一バックエンドだけを使う。`STORAGE_BACKEND` を変更すると、変更前のファイルを取得・削除できない |
| 中 | 共有トークンのレート制限 | 共有トークンは十分長いが、公開共有 API 専用のレートリミットは未実装 |
| 低 | 未使用の S3 公開 URL 設定 | 実装は `S3_PUBLIC_BASE_URL` を読み込むが、全ファイルをプロキシ配信するため値は使われない |
| 未実装 | フロントエンド | Drive ページ、ファイルブラウザ、アップロード、クォータ、共有 UI は未実装 |

上記に加え、公開共有 API を直接検証する自動テストと、フォルダ CRUD・プロジェクト境界の
統合テストが不足している。現在ある Drive 統合テストは主に本文更新・本文配信・クォータ・
プロジェクトフォルダへの直接アップロードを対象としている。

---

## 13. 今後の実装順序

バックエンドの基本 API は揃っているため、先にデータ境界とストレージ整合性を直し、
その契約をテストで固定してからフロントエンドへ進む。

| 順序 | 内容 | 完了条件 |
|------|------|----------|
| 1 | 公開共有ルートの prefix 修正 | 仕様どおりの2エンドポイントを統合テストし、OpenAPI を再生成する |
| 2 | フォルダ階層の `project_id` 継承と ACL 修正 | 作成・移動・削除、異なるプロジェクト間、共有継承の境界テストが通る |
| 3 | S3 ストリーミング修正 | 設定上限近くのファイルでも全量バッファせず multipart upload になる |
| 4 | ストレージ削除の回収経路追加 | 個別・プロジェクト・テナント削除と失敗時再試行をテストできる |
| 5 | 共有受信者向け一覧 API の整理 | テナント所属に依存せず、共有範囲だけを列挙できる |
| 6 | ストレージバックエンド移行方針の決定 | `storage_type` ごとに取得するか、切り替え前の移行を必須化する |
| 7 | フロントエンド実装 | ファイルブラウザ、アップロード、本文編集、クォータ、共有 UI を提供する |

---

## 14. 決定事項ログ

| 項目 | 決定内容 | 決定日 |
|------|---------|--------|
| ファイルサイズ上限 | `UPLOAD_MAX_SIZE_MB` 環境変数で設定変更可（デフォルト 100MB） | 2026-05-26 |
| 一般ファイルの配信認証 | テナント所属者・オーナー、または有効なフォルダ共有トークンのみ | 2026-08-10 |
| プロジェクトファイルのアクセス制御 | プロジェクトメンバーまたはテナントオーナーのみ閲覧可 | 2026-05-26 |
| プロジェクトファイルのアクセス制御（改訂） | テナントに所属していることを先に確認したうえで、プロジェクトの公開規則で判定する | 2026-08-18 |
| プロジェクトファイルの URL 戦略 | バックエンド経由でプロキシ（S3 URL は直接渡さない） | 2026-05-26 |
| S3 バケット公開設定 | 公開を前提としない。全ファイルをバックエンドプロキシで配信 | 2026-09-03 |
| フォルダ削除の挙動 | ファイルまたは子フォルダが存在する場合は `409 Conflict`（強制削除は将来対応） | 2026-09-03 |
| MIME タイプ制限 | 現行 API は全種類許可 | 2026-09-03 |
| S3 ForcePathStyle | `S3_FORCE_PATH_STYLE` 環境変数で設定可（MinIO 等向け） | 2026-05-26 |
| プロジェクトフォルダ自動作成 | プロジェクト作成時に対応するドライブフォルダを自動作成 | 2026-05-26 |
| テナント別ドライブ容量 | `tenants.drive_quota_bytes` で個別設定。`NULL` 時は `DRIVE_DEFAULT_QUOTA_MB` 適用（`0` = 無制限） | 2026-05-26 |
| システム上限（ハードキャップ） | `DRIVE_SYSTEM_MAX_QUOTA_MB` で設定。テナントオーナーはこの値を超えて設定不可。`0` = 天井なし | 2026-05-26 |
| クォータ超過時のレスポンス | `413 Content Too Large` | 2026-05-26 |
| システム上限違反時のレスポンス | `400 Bad Request` | 2026-05-26 |
| フォルダ共有 | ユーザー指定共有 + 公開リンク共有の 2 種類。配下サブフォルダ・ファイルに再帰継承 | 2026-05-26 |
| 共有権限 | 現行は `viewer`（閲覧のみ）だけを実装。`editor` は将来対応 | 2026-09-03 |
| 公開リンクトークン | 32 文字 URL-safe ランダム。有効期限設定可（NULL = 無期限）| 2026-05-26 |
| PAT スコープ | `read:drive`（閲覧）・`write:drive`（書き込み）を新設。`write:drive` は `read:drive` を包含。`admin:tenant` は全 Drive 操作を包含 | 2026-05-26 |
| ストリーミング | `StorageBackend` trait は `BoxStream` を使用する。S3 の全量バッファは既知の修正対象 | 2026-09-03 |
| URL 管理 | `drive_files.url` カラム廃止。全ファイルを `/v1/drive/files/{id}/content` で統一配信 | 2026-05-26 |
| 不変条件保護 | `project_id IS NULL OR folder_id IS NOT NULL` を DB CHECK 制約 + アプリバリデーション両方で強制 | 2026-05-26 |
| editor 権限 | `editor` を指定した場合は `422 Unprocessable Entity`。実装は将来対応 | 2026-09-03 |
| share_url | バックエンドは `share_token` のみ返す。フロントが `window.location.origin` で URL を組み立てる | 2026-05-26 |
| 上限引き下げ時の挙動 | DB の個別設定値は変更せず起動時に警告。実効クォータは新しいシステム上限で cap する | 2026-09-03 |
| タスク添付 | Drive ファイルとタスクの紐付け・解除 API は実装済み | 2026-09-03 |
