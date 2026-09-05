---
title: OAuth 認証拡張
description: GitHub・GitLab・汎用 OIDC による既存認証基盤への OAuth ログイン追加
icon: lucide:key-round
---

# OAuth 認証拡張 仕様書

> ステータス: **Draft** / 作成日: 2026-05-27
> **依存: なし（既存の `users` + `sessions` 基盤を拡張）**

---

## 1. 概要

既存のメール + パスワード認証（Argon2id）に加え、GitHub・GitLab・汎用 OIDC（Google 等）でのログイン・新規登録を可能にする。

設計方針:
- **Authorization Code + PKCE** のみ（Implicit Flow 禁止）
- OAuth ユーザーもセッション管理は既存の Redis セッションと共通
- メール/パスワードユーザーは後から OAuth を連携できる（逆も可）
- パスワードなしの OAuth 専用ユーザーも許容する

---

## 2. データモデル変更

### 2.1 `users` テーブル変更

OAuth ユーザーはパスワードを持たないため `password_hash` を NULL 許容にする。

```sql
ALTER TABLE users ALTER COLUMN password_hash DROP NOT NULL;
```

> 既存の全ユーザーはパスワードハッシュを持つため、NULL 許容化はデータ上の破壊的変更ではない。  
> ログイン時にパスワードが NULL のユーザーへのパスワード認証は `401 invalid-credentials` を返す。

### 2.2 `oauth_connections`（新規）

```rust
pub struct Model {
    pub id: Uuid,
    pub user_id: Uuid,
    pub provider: String,           // "github" | "gitlab" | "google" | "oidc:{issuer}"
    pub provider_user_id: String,   // プロバイダー側のユーザー ID
    pub provider_email: Option<String>, // プロバイダーが返したメール（参照用）
    pub access_token_enc: Option<String>,  // AES-256-GCM 暗号化
    pub refresh_token_enc: Option<String>, // AES-256-GCM 暗号化
    pub token_expires_at: Option<DateTimeUtc>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}
```

| カラム | 型 | 制約 | 説明 |
|--------|-----|------|------|
| `id` | UUID | PK | |
| `user_id` | UUID | NOT NULL, FK→users CASCADE | |
| `provider` | VARCHAR | NOT NULL | `github` / `gitlab` / `gitlab_selfhosted` / `google` / `oidc:{issuer}` |
| `provider_user_id` | VARCHAR | NOT NULL | プロバイダー側の不変ユーザー ID |
| `provider_email` | VARCHAR | NULLABLE | プロバイダーが返したメール（参照用。主キーとして使わない） |
| `instance_url` | VARCHAR | NULLABLE | GitLab self-hosted のインスタンス URL（例: `https://gitlab.example.com`）。クラウド版は NULL |
| `access_token_enc` | TEXT | NULLABLE | AES-256-GCM 暗号化 |
| `refresh_token_enc` | TEXT | NULLABLE | AES-256-GCM 暗号化 |
| `token_expires_at` | TIMESTAMPTZ | NULLABLE | |
| `created_at` | TIMESTAMPTZ | NOT NULL DEFAULT now() | |
| `updated_at` | TIMESTAMPTZ | NOT NULL DEFAULT now() | |
| — | — | UNIQUE(provider, provider_user_id, instance_url) | self-hosted 複数インスタンス対応 |

---

## 3. マイグレーション

```sql
ALTER TABLE users ALTER COLUMN password_hash DROP NOT NULL;

CREATE TABLE oauth_connections (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider VARCHAR NOT NULL,
    provider_user_id VARCHAR NOT NULL,
    provider_email VARCHAR,
    instance_url VARCHAR,
    access_token_enc TEXT,
    refresh_token_enc TEXT,
    token_expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (provider, provider_user_id, instance_url)
);

CREATE INDEX idx_oauth_connections_user ON oauth_connections(user_id);
```

---

## 4. 対応プロバイダー

| プロバイダー | `provider` 値 | 方式 | 備考 |
|------------|--------------|------|------|
| GitHub | `github` | OAuth 2.0 | GitHub Apps とは別。`user:email` スコープのみ |
| GitLab.com | `gitlab` | OAuth 2.0 | `read_user` スコープ |
| GitLab self-hosted | `gitlab_selfhosted` | OAuth 2.0 | ユーザーがインスタンス URL を入力（§4a 参照） |
| Google | `google` | OIDC | `openid email profile` スコープ |
| 汎用 OIDC | `oidc:{issuer}` | OIDC Discovery | Okta・Entra ID・Keycloak 等に対応 |

プロバイダー設定は環境変数で管理する（テナント単位の設定は本仕様の対象外）:

```env
OAUTH_GITHUB_CLIENT_ID=...
OAUTH_GITHUB_CLIENT_SECRET=...
OAUTH_GITLAB_CLIENT_ID=...
OAUTH_GITLAB_CLIENT_SECRET=...
OAUTH_GITLAB_SELFHOSTED_CLIENT_ID=...
OAUTH_GITLAB_SELFHOSTED_CLIENT_SECRET=...
OAUTH_GOOGLE_CLIENT_ID=...
OAUTH_GOOGLE_CLIENT_SECRET=...
# 汎用 OIDC（複数設定可）
OAUTH_OIDC_ISSUER_URL=https://accounts.example.com
OAUTH_OIDC_CLIENT_ID=...
OAUTH_OIDC_CLIENT_SECRET=...
```

### §4a. GitLab self-hosted フロー

GitLab.com と異なり、OAuth エンドポイントがインスタンスごとに異なる。

```text
1. ユーザーがログイン画面で「GitLab self-hosted でログイン」を選択し、
   インスタンス URL（例: https://gitlab.example.com）を入力

2. GET /v1/auth/oauth/gitlab_selfhosted?instance_url={url} をリクエスト
   → バックエンドが instance_url を Redis の state に紐付けて保存
   → 認可 URL: {instance_url}/oauth/authorize?...

3. コールバック: GET /v1/auth/oauth/gitlab_selfhosted/callback
   → state から instance_url を復元
   → トークンエンドポイント: {instance_url}/oauth/token
   → ユーザー情報: {instance_url}/api/v4/user
   → oauth_connections.instance_url に保存

4. 同一ユーザーが複数の self-hosted インスタンスを接続可能
   （UNIQUE(provider, provider_user_id, instance_url) で識別）
```

**OAuth アプリの事前登録**: self-hosted 利用には、対象インスタンスに管理者がシステムの OAuth アプリ（Client ID / Secret）を登録しておく必要がある。`callback URL` は `{APP_BASE_URL}/v1/auth/oauth/gitlab_selfhosted/callback` に統一する。

---

## 5. OAuth フロー（Authorization Code + PKCE）

```text
1. クライアントが GET /v1/auth/oauth/{provider} をリクエスト
2. バックエンドが生成:
   - code_verifier  (32 バイト乱数, base64url)
   - code_challenge (SHA-256(code_verifier), base64url)
   - state          (16 バイト乱数, base64url)
3. Redis に state をキー、{code_verifier, redirect_after} を値として保存 (TTL: 10 分)
4. 認可 URL へリダイレクト:
   ?response_type=code&client_id=...&scope=...
    &redirect_uri=...&state={state}&code_challenge={ch}&code_challenge_method=S256
5. ユーザーがプロバイダーで認可
6. プロバイダーが GET /v1/auth/oauth/{provider}/callback?code={code}&state={state} へリダイレクト
7. バックエンドが state を検証 (Redis から取得して一致確認、即削除)
8. code + code_verifier でトークンエンドポイントへ POST → access_token 取得
9. プロバイダーの userinfo API を叩いてユーザー情報を取得
10. ユーザー照合・作成（§6 参照）
11. Redis セッション発行 → クライアントにセッション Cookie をセット
12. redirect_after へリダイレクト（デフォルト: /dashboard）
```

`redirect_after` は `/` で始まる相対パスのみ許可する（`//`・`:`・`@`・`..` を拒否）。組み立て時はフロント基底 URL と `url::Url::join` で同一 origin を検証する。プロバイダーが `error` を返した場合は詳細をログに残し、フロントへ `oauth_error=authorization_failed` のみ付与してリダイレクトする。

---

## 6. ユーザー照合ロジック

コールバック受信時、プロバイダーから `provider_user_id` と `provider_email` を受け取る。

```text
A) oauth_connections に (provider, provider_user_id) が存在する
   → 既存ユーザーとしてログイン。access_token を更新。

B) 存在しない かつ provider_email が users.email と一致するユーザーがいる
   → "メール競合" → §6.1 参照

C) 存在しない かつ メール一致なし
   → 新規ユーザーを作成し oauth_connections も INSERT（§6.2 参照）
```

### 6.1 メール競合（ケース B）

既存のメール/パスワードユーザーと同じメールアドレスで OAuth ログインを試みた場合:

- **既存セッションあり（ログイン済み）**: oauth_connections を追加で INSERT → 連携完了
- **既存セッションなし**: `409 Conflict` を返し、フロントエンドが「このメールアドレスは既に登録されています。ログインして連携してください。」を表示

> プロバイダーが返すメールアドレスは不変ではなく、なりすまし防止のため自動マージはしない。

### 6.2 新規ユーザー作成（ケース C）

```rust
users::ActiveModel {
    id: Set(Uuid::new_v4()),
    username: Set(derive_username_from_provider(provider_info)),
    email: Set(provider_email),
    email_verified: Set(provider_info.email_verified.unwrap_or(false)), // プロバイダーの検証フラグを反映
    password_hash: Set(None),    // OAuth ユーザーはパスワードなし
    bio: Set(Some(String::new())),
    avatar_url: Set(provider_info.avatar_url),
}
```

ユーザー名は `{provider_username}` をベースに、重複時は `{name}_2`, `{name}_3` ... とする。

---

## 7. アカウント連携・解除

### 連携（ログイン済みユーザーが追加 OAuth を接続する）

1. ログイン済みで `GET /v1/auth/oauth/{provider}` をリクエスト（セッションあり）
2. コールバック受信後、現在のセッションユーザーに oauth_connections を追加
3. 同じプロバイダーが既に連携済みなら `409`

### 解除

```text
DELETE /v1/auth/oauth/connections/{provider}
```

**ガード**: 最後の認証手段を解除できない:

```text
(oauth_connections が 1 件のみ) AND (password_hash IS NULL)
→ 403: パスワードを設定してから解除してください
```

---

## 8. API

| メソッド | パス | 認証 | 説明 |
|---------|------|------|------|
| `GET` | `/v1/auth/oauth/{provider}` | 不要 | 認可 URL へリダイレクト |
| `GET` | `/v1/auth/oauth/{provider}/callback` | 不要 | コールバック処理（公開エンドポイント） |
| `GET` | `/v1/auth/oauth/connections` | セッション必須 | 連携済みプロバイダー一覧 |
| `DELETE` | `/v1/auth/oauth/connections/{provider}` | セッション必須 | 連携解除 |
| `POST` | `/v1/auth/password` | セッション必須 | OAuth ユーザーがパスワードを新規設定 |

`GET /v1/auth/oauth/connections` レスポンス:

```json
{
  "connections": [
    {
      "provider": "github",
      "provider_email": "user@example.com",
      "connected_at": "2026-05-27T10:00:00Z"
    }
  ]
}
```

`POST /v1/auth/password`（OAuth ユーザーのパスワード初回設定）:

```json
{ "password": "new-password-min-8" }
```

> 既にパスワードが設定されている場合は `409`。変更は `/v1/auth/password/change` を使用（[パスワードリセット仕様書](/features/auth-password-reset) §4 参照）。

---

## 9. セキュリティ

| 脅威 | 対策 |
|------|------|
| CSRF | `state` パラメータを Redis で管理（TTL 10 分、コールバック後即削除） |
| Code Injection | PKCE (`code_challenge_method=S256`)。`code_verifier` は Redis にのみ存在 |
| Token 漏洩 | `access_token` / `refresh_token` は AES-256-GCM で暗号化して保存。API レスポンスでは返さない |
| メール偽装 | プロバイダーのメール一致でも自動マージしない。セッションなし時は `409` |
| provider_user_id なりすまし | `UNIQUE(provider, provider_user_id)` で保証。プロバイダー跨ぎで ID が衝突しても無効 |
| 最後の認証手段削除 | 解除前に password_hash と残 connections 数を確認 |

---

## 10. フロントエンド（Phase B）

### ログイン画面

```text
┌─────────────────────────────────────────────┐
│ ログイン                                     │
├─────────────────────────────────────────────┤
│ メールアドレス [___________________]         │
│ パスワード     [___________________]         │
│                             [ログイン]       │
│                                             │
│ ─────────── または ────────────              │
│                                             │
│ [  GitHub でログイン  ]                      │
│ [  GitLab でログイン  ]                      │
│ [  Google でログイン  ]                      │
└─────────────────────────────────────────────┘
```

### アカウント設定「認証方法」

連携の一覧・追加・解除は、パスワードと同じ「サインインできる方法」として
`/settings/security` にまとめた。どちらも「最低 1 つは残す」という同じ制約を共有するため、
別々の画面にしていない。画面の仕様は
[アカウント設定](/features/account-settings) §7 を正とする。

```text
/settings/security
```

```text
┌─────────────────────────────────────────────┐
│ 認証方法                                      │
├─────────────────────────────────────────────┤
│ パスワード  設定済み          [パスワードを変更] │
│ GitHub     user@example.com  [解除]          │
├─ 追加できる連携 ─────────────────────────────┤
│ Google     —                 [連携する]      │
└─────────────────────────────────────────────┘
```

連携の追加は既存の開始 URL（`GET /v1/auth/oauth/{provider}`）をそのまま使い、
`redirect_after` にこの画面を指定して戻す。ログイン済みで開始した場合の連携追加は §6.3 の経路。

### コンポーネント

| コンポーネント | ファイル |
|--------------|---------|
| `OAuthButtons` | `components/auth/OAuthButtons.vue` |
| `AuthMethodsSection` | `components/settings/AuthMethodsSection.vue` |
| `PasswordMethodRow` | `components/settings/PasswordMethodRow.vue` |
| プロバイダー表示名・開始 URL | `lib/oauth-providers.ts`（サインイン画面と共通） |

---

## 11. 未決事項

| 項目 | 内容 |
|------|------|
| テナント単位 OIDC | 企業向けに Okta 等を「テナント独自の IdP」として設定できるようにするか（SaaS 標準機能） |
| アカウント統合 UI | メール競合時に「ログインして連携」を seamless に誘導するフロー設計 |
