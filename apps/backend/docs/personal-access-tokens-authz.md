# パーソナルアクセストークン（PAT）認可

**関連コード**: `src/entities/personal_tokens.rs`, `src/entities/scopes.rs`, `src/extractors.rs`, `src/utils/auth.rs`

## 概要

PAT による API 認証と、テナント・プロジェクトを横断しない権限チェックの設計・実装を記述する。

- **セッション（Cookie）**: 全操作スコープ相当。テナント切り替えは UI から可能。
- **PAT（Bearer）**: 作成時に固定した **1 テナント** と **任意のプロジェクト一覧** の範囲内でのみ有効。操作は `scopes` で制限。

認可は **操作スコープ（何ができるか）** と **リソース束縛（どこでできるか）** の 2 層で行う。

## 認証方式ごとの動作

| 認証方式 | 操作スコープ | リソース範囲 |
|----------|-------------|--------------|
| セッション | `require_scope` は常に通過 | 所属しているテナント（オーナー or テナントメンバー） |
| PAT | DB の `scopes` を検証 | 作成時の `tenant_id` のみ。プロジェクトは任意指定 |

**バインドは所属の証明ではない。** PAT の `tenant_id` は「どのテナントを触れるか」の上限であって、
その利用者がいまテナントに所属していることを意味しない。
所属判定はセッションと同じ経路（`has_tenant_access`）を通るため、テナントから外した利用者のトークンは
バインド先のテナントでも 403 になる。詳細は [tenant-project-authz.md](./tenant-project-authz.md) を参照。

## PAT のリソース束縛

| 項目 | 方針 |
|------|------|
| テナント | 作成時に **1 件必須**（`tenant_id`）。他テナントの API は 403 |
| プロジェクト | **任意指定**（`allowed_project_ids`）。未指定（`NULL`）= 当該テナント内の全プロジェクト |
| 複数テナント PAT | 採用しない |
| テナント非紐づけ PAT | 採用しない（`/me` 等アカウント API はセッション専用） |
| テナント作成 | **セッション専用**（PAT はテナントにバインドされているため新規作成不可） |
| PAT 管理 API（一覧・作成・失効） | **セッション専用**（PAT では不可）。一覧は自分の `revoked = false` のトークンだけを返す |

## 二層モデル

### Layer 1: 操作スコープ

「何ができるか」を `personal_tokens.scopes`（`ScopeList`）で表現する。

| スコープ | 意味 |
|---------|------|
| `read:project` | プロジェクトの参照 |
| `write:project` | プロジェクトの更新 |
| `admin:tenant` | 当該 PAT の `tenant_id` 内の管理操作（wildcard） |

`admin:tenant` を持つトークンはすべての `require_scope` チェックを通過する（`ScopeList::has_scope` 参照）。

スコープ文字列にテナント ID を埋め込まない（例: `read:tenant:uuid` は採用しない）。  
`/me` 等アカウント API はセッション専用のため `read:user` / `write:user` は存在しない。

### Layer 2: リソース束縛

「どこでできるか」を DB カラムで表現する。

| カラム | 型 | 説明 |
|--------|-----|------|
| `tenant_id` | `UUID` NOT NULL | PAT が有効なテナント（1 件固定） |
| `allowed_project_ids` | `JSON` NULL 可 | 許可プロジェクト ID の配列。`NULL` = テナント内全プロジェクト |

## 認証・認可の実装

### データ構造

```rust
pub enum AuthMethod {
    Session,
    PersonalToken {
        token_id: Uuid,
        tenant_id: Uuid,
        allowed_project_ids: Option<Vec<Uuid>>,
        scopes: ScopeList,
    },
}

pub struct AuthUser {
    pub user_id: Uuid,
    pub method: AuthMethod,
}
```

### 認証フロー（`FromRequestParts`）

1. `Authorization: Bearer <token>` があれば PAT 認証
2. なければ Cookie セッションから `user_id` 取得
3. どちらもなければ **401**

PAT 認証（`authenticate_personal_token` in `utils/auth.rs`）:

1. Bearer トークンを HMAC-SHA256 ハッシュ化（`PERSONAL_TOKEN_SECRET` は `Settings` 経由で起動時に検証済み）
2. `personal_tokens` を `token_hash` で 1 SELECT
3. 失効・期限切れチェック
4. `AuthMethod::PersonalToken { ... }` を構築（**以降の認可はメモリのみ**）

> **未実装**: `last_used_at` の fire-and-forget 更新

### エンドポイントでの認可

ハンドラ先頭でヘルパーを呼ぶ。スコープ専用エクストラクタは作らない。

```rust
// テナント + プロジェクト両方を守る場合
auth.require_scope(Scope::ReadProject)?;
auth.ensure_tenant_access(&state, tenant_id, Some(project_id)).await?;

// テナントオーナー専用操作
auth.require_scope(Scope::AdminTenant)?;
let tenant = auth.ensure_tenant_owner(&state, tenant_id).await?;
```

### 認可ヘルパー

| メソッド | セッション | PAT |
|----------|-----------|-----|
| `require_scope` | 常に OK | `scopes` を検証。不足なら 403 |
| `ensure_tenant_access` | 所属判定（`has_tenant_access`） | `token.tenant_id == path.tenant_id` + `allowed_project_ids` の突き合わせ（メモリ）の**あと**、セッションと同じ所属判定 |
| `ensure_tenant_owner` | `owner_id` チェック（1 SELECT） | `token.tenant_id` 一致 + プロジェクト制限なし + `owner_id` チェック |

### HTTP ステータス

| 状況 | ステータス |
|------|-----------|
| 未認証・無効 PAT・失効・期限切れ | 401 |
| 認証済みだがスコープ / テナント / プロジェクト不一致 | 403 |

## API パス規約

テナント配下のリソースは path にテナント（・プロジェクト）を含める。

```
GET /v1/tenants/{tenant_id}/projects/{project_id}/...
```

path の ID と PAT の `tenant_id` / `allowed_project_ids` を突き合わせる。  
アカウント API（例: `GET /v1/auth/me`）は PAT 非対応（セッションのみ）。

## DB アクセス回数

| 認証 | 認可まわりの DB（目安） |
|------|------------------------|
| PAT | 認証時 `personal_tokens` **1 SELECT**。`require_scope` とバインドの突き合わせは追加クエリなし。所属判定はセッションと同じクエリを流す |
| セッション | Redis セッション + 所属判定（テナント 1 SELECT + `tenant_members` 1 SELECT。プロジェクト指定時は最大 3 SELECT 追加） |

避けること:

- `require_*` ごとに DB を再クエリする
- ハンドラごとに Bearer パース + トークン lookup を重複する

## テスト要件

- `ScopeList::has_scope`: `admin:tenant` は全スコープを通過、不足スコープは 403
- `require_scope`: Session は常に OK、PAT は不足で 403
- PAT が別テナントの path を叩く → 403
- `allowed_project_ids` 外の project → 403、`NULL` ならテナント内任意 project → OK
- 失効 / 期限切れ / ハッシュ不一致 → 401
- PAT 作成: 他人の `tenant_id` → 403

## 採用しない方針

- セッションにスコープを持たせる
- 複数テナント / テナント非紐づけ PAT
- スコープ専用エクストラクタ
- Route Layer での一括スコープ管理
- スコープ文字列へのリソース ID 埋め込み
- `scopes` だけでテナント境界を守る（必ず `tenant_id` カラムと併用）

## 参考

- GitHub: PAT scopes + fine-grained PAT の repository 指定
- GitLab: project access token
