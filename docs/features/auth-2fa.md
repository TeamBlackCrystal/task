---
title: 2段階認証（2FA）
description: TOTP・リカバリーコード・テナント強制ポリシー
icon: lucide:shield-check
---

# 2段階認証（2FA）仕様書

> ステータス: **Draft** / 作成日: 2026-05-27
> **依存: なし（`users` + `sessions` 基盤を拡張。[OAuth 拡張](/features/auth-oauth) と独立）**

---

## 1. 概要

メール/パスワードおよび OAuth ログイン後に TOTP（Time-based One-Time Password）による第二認証を追加する。

**2FA が免除されるケース:**
- **パスキーログイン**は「所持 + 生体」の多要素認証であるため、TOTP を要求しない（[パスキー仕様書](/features/auth-passkeys) 参照）

**テナント強制ポリシー:**
- テナントオーナーは全メンバーに 2FA を強制できる
- 強制後に未設定のユーザーはログイン後に 2FA 設定を完了するまで他 API を使用不可

実装ライブラリ: [`totp-rs`](https://github.com/constantoine/totp-rs)

---

## 2. データモデル

### 2.1 `totp_credentials`

| カラム | 型 | 制約 | 説明 |
|--------|-----|------|------|
| `user_id` | UUID | PK, FK→users CASCADE | 1 ユーザー 1 シークレット |
| `secret_enc` | TEXT | NOT NULL | AES-256-GCM 暗号化済み TOTP シークレット（Base32） |
| `is_verified` | BOOLEAN | NOT NULL DEFAULT false | 初回コード入力で `true` になるまで有効扱いしない |
| `created_at` | TIMESTAMPTZ | NOT NULL DEFAULT now() | |

### 2.2 `recovery_codes`

| カラム | 型 | 制約 | 説明 |
|--------|-----|------|------|
| `id` | UUID | PK | |
| `user_id` | UUID | NOT NULL, FK→users CASCADE | |
| `code_hash` | VARCHAR | NOT NULL | HMAC-SHA256(secret, code) のハッシュ |
| `used_at` | TIMESTAMPTZ | NULLABLE | NULL = 未使用 |
| `created_at` | TIMESTAMPTZ | NOT NULL DEFAULT now() | |

### 2.3 `users` テーブル変更

```sql
ALTER TABLE users ADD COLUMN totp_enabled BOOLEAN NOT NULL DEFAULT false;
```

> `totp_enabled = true` かつ `totp_credentials.is_verified = true` の場合に 2FA が有効。  
> 無効化時は `totp_enabled = false` + `totp_credentials` + `recovery_codes` を削除。

### 2.4 `tenants` テーブル変更（強制ポリシー）

```sql
ALTER TABLE tenants ADD COLUMN require_2fa BOOLEAN NOT NULL DEFAULT false;
```

---

## 3. マイグレーション

```sql
ALTER TABLE users ADD COLUMN totp_enabled BOOLEAN NOT NULL DEFAULT false;

ALTER TABLE tenants ADD COLUMN require_2fa BOOLEAN NOT NULL DEFAULT false;

CREATE TABLE totp_credentials (
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    secret_enc TEXT NOT NULL,
    is_verified BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE recovery_codes (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    code_hash VARCHAR NOT NULL,
    used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_recovery_codes_user ON recovery_codes(user_id);
```

---

## 4. TOTP セットアップフロー

```text
1. POST /v1/auth/2fa/totp/setup
   ← セッション必須（ログイン済みユーザー）
   → サーバーが TOTP シークレット（Base32, 20 バイト）を生成
   → secret_enc を DB に保存（is_verified = false）
   → otpauth:// URI と QR コード用 Base64 PNG を返す

2. ユーザーが認証アプリ（Google Authenticator・Authy・1Password 等）でQRを読み込む

3. POST /v1/auth/2fa/totp/verify-setup
   Request: { "code": "123456" }
   → TOTP コード検証（前後 1 ステップ許容）
   → 成功: is_verified = true, users.totp_enabled = true
   → リカバリーコードを 10 個生成して返す（この 1 回のみ平文表示）
```

`POST /v1/auth/2fa/totp/setup` レスポンス:

```json
{
  "otpauth_uri": "otpauth://totp/TaskApp:user@example.com?secret=BASE32SECRET&issuer=TaskApp",
  "qr_code_png": "data:image/png;base64,..."
}
```

`POST /v1/auth/2fa/totp/verify-setup` レスポンス（初回設定時のみリカバリーコードを返す）:

```json
{
  "recovery_codes": [
    "XXXX-XXXX-XXXX",
    "YYYY-YYYY-YYYY",
    "..."
  ]
}
```

> リカバリーコードはこの 1 回しか表示されない。ユーザーに必ず保存を促すこと。

---

## 5. ログイン時の 2FA フロー（半セッション）

2FA が有効なユーザーがメール/パスワードまたは OAuth でログインした場合:

```text
1. 第一認証成功（パスワード検証 or OAuth コールバック）
   → Redis セッションに { user_id, half_authed: true } を保存
   → 200 OK  { "requires_2fa": true }

2. クライアントが POST /v1/auth/2fa/verify へ TOTP コード or リカバリーコードを送信

3. 検証成功
   → Redis セッションを { user_id, half_authed: false } に更新（完全認証）
   → 204 No Content

4. 通常の API リクエスト
   → AuthUser extractor が half_authed: true を検出したら 403 を返す
```

**セッション状態の遷移:**

```text
[未ログイン]
    │ POST /v1/login (password OK)
    ▼
[half_authed=true]  ← この状態では /v1/auth/2fa/verify 以外の API は 403
    │ POST /v1/auth/2fa/verify (code OK)
    ▼
[half_authed=false]  ← 通常の認証済み状態
```

### エクストラクタ責務分離設計

`apps/backend/src/extractors.rs` に以下 3 つのエクストラクタを定義する。既存の `AuthUser` を直接変更すると PAT 認証経路や既存 API に影響が及ぶため、責務を分離して段階的に追加する。

| エクストラクタ | 受理する認証 | `half_authed` 確認 | 用途 |
|--------------|------------|-------------------|------|
| `AuthUser` | セッション / PAT | セッション時のみ確認 | 通常の保護済み API 全般 |
| `HalfAuthedUser` | セッションのみ | `true` を必須とする | `/v1/auth/2fa/verify` のみ |
| `AdminUser` | セッションのみ | `false` を必須とする | `/v1/admin/**` |

**`AuthUser` への変更方針:**

- PAT 認証の場合は `half_authed` チェックをスキップ（PAT はセッションを持たず、常に完全認証扱い）
- セッション認証の場合のみ `half_authed` フラグを確認し、`true` なら `403`

```rust
// AuthUser::from_request_parts 内（セッション認証パスのみ追加）
let half_authed = session.get::<bool>("half_authed").unwrap_or(false);
if half_authed {
    return Err(AuthError::Forbidden);
}
```

**`HalfAuthedUser` エクストラクタ:**

`/v1/auth/2fa/verify` 専用。`half_authed: true` のセッションのみ受理し、`false` または PAT は `403`。

```rust
pub struct HalfAuthedUser {
    pub user_id: Uuid,
}

impl FromRequestParts<AppState> for HalfAuthedUser {
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let session = session_from_request_parts(parts, state).await?;  // PAT は不可（セッションのみ）
        let user_id = session.get::<Uuid>("user_id").ok_or(AuthError::Unauthorized)?;
        let half_authed = session.get::<bool>("half_authed").unwrap_or(false);
        if !half_authed {
            return Err(AuthError::Forbidden);
        }
        Ok(HalfAuthedUser { user_id })
    }
}
```

---

## 6. TOTP 検証仕様

| 項目 | 値 |
|------|----|
| アルゴリズム | HMAC-SHA1（RFC 6238 標準） |
| 桁数 | 6 桁 |
| ステップ | 30 秒 |
| 許容スキュー | ±1 ステップ（±30 秒） |
| ブルートフォース対策 | 5 回連続失敗で 15 分ロック（Redis カウンター） |

---

## 7. リカバリーコード

- 生成数: 10 個
- 形式: `XXXX-XXXX-XXXX`（大文字英数字 12 文字、ハイフン区切り）
- 保存: HMAC-SHA256(server_secret, code) のハッシュのみ DB に保存（平文は保存しない）
- 使用: 1 回使用したら `used_at` を SET（再利用不可）
- 再生成: 新しいコードを 10 個生成し古いものを全削除（残数が不安な場合に使用）

`POST /v1/auth/2fa/verify` リクエスト（TOTP またはリカバリーコードのどちらか）:

```json
{ "code": "123456" }
```

または

```json
{ "recovery_code": "XXXX-XXXX-XXXX" }
```

---

## 8. 2FA 無効化

```text
DELETE /v1/auth/2fa/totp
Request: { "code": "123456" }  (現在の TOTP コード or リカバリーコードが必要)
```

処理:
1. コードを検証
2. `users.totp_enabled = false`
3. `totp_credentials` を DELETE
4. `recovery_codes` を全 DELETE

---

## 9. テナント強制ポリシー

テナントオーナーが `require_2fa = true` に設定した場合:

```text
POST /v1/tenants/{tenant_id}/require-2fa
Request: { "enabled": true }
権限: テナントオーナーのみ
```

**強制後のフロー:**

```text
ログイン（パスワード/OAuth）
    │ 該当テナントで require_2fa = true
    │ かつ users.totp_enabled = false
    ▼
half_authed セッション + { "requires_2fa_setup": true }
    │ POST /v1/auth/2fa/totp/setup → verify-setup
    ▼
通常認証済みセッション
```

2FA 設定完了前はそのテナントのリソースへのアクセスを `403` で拒否する。  
パスキーログインユーザーはテナント強制ポリシーの対象外（パスキー自体が MFA）。
project にだけ参加しテナントに参加しない客分（project-only guest）も、関わるテナントの
`require_2fa` の対象である（ここから漏れると、除名後に残る `project_members` の行で
project へ入れる利用者が 2FA 無しで読み書きできてしまう）。

---

## 10. API

| メソッド | パス | 認証 | 説明 |
|---------|------|------|------|
| `POST` | `/v1/auth/2fa/totp/setup` | セッション必須 | TOTP シークレット生成・QR 返却 |
| `POST` | `/v1/auth/2fa/totp/verify-setup` | セッション必須 | 初回コード検証→有効化 |
| `POST` | `/v1/auth/2fa/verify` | half_authed セッション | ログイン後の TOTP / リカバリーコード検証 |
| `DELETE` | `/v1/auth/2fa/totp` | セッション必須 | 2FA 無効化（コード要求） |
| `POST` | `/v1/auth/2fa/recovery-codes/regenerate` | セッション必須 | リカバリーコード再生成（コード要求） |
| `POST` | `/v1/tenants/{tenant_id}/require-2fa` | テナントオーナー | テナント 2FA 強制ポリシー変更 |

---

## 11. セキュリティ

| 脅威 | 対策 |
|------|------|
| ブルートフォース | 5 回失敗で 15 分ロック（Redis: `2fa_attempts:{user_id}`） |
| リカバリーコード漏洩 | HMAC-SHA256 ハッシュのみ保存。平文は生成時の 1 回のみ表示 |
| TOTP シークレット漏洩 | AES-256-GCM で暗号化して保存。API で返却しない |
| 半セッションの悪用 | `AuthUser` extractor が `half_authed: true` を検出したら即 `403`（§5 参照） |
| タイミング攻撃 | リカバリーコード検証は `constant_time_eq` で比較 |

---

## 12. フロントエンド（Phase B）

### ログイン後の 2FA 入力画面

```text
┌─────────────────────────────────────────────┐
│ 2段階認証                                    │
├─────────────────────────────────────────────┤
│ 認証アプリのコードを入力してください           │
│                                             │
│  [ _ _ _ _ _ _ ]                            │
│                             [確認]           │
│                                             │
│  リカバリーコードを使用                       │
└─────────────────────────────────────────────┘
```

### セキュリティ設定画面

```text
/settings/security
```

```text
┌─────────────────────────────────────────────┐
│ 2段階認証                        [有効 ✅]   │
├─────────────────────────────────────────────┤
│ 認証アプリ: 設定済み                         │
│                                             │
│ リカバリーコード: 残り 8 / 10                │
│ [リカバリーコードを再生成]                   │
│                                             │
│ [2段階認証を無効にする]                      │
└─────────────────────────────────────────────┘
```

| コンポーネント | ファイル |
|--------------|---------|
| `TwoFactorPrompt` | `components/auth/TwoFactorPrompt.vue` |
| `TotpSetupWizard` | `components/settings/TotpSetupWizard.vue` |
| `RecoveryCodeDisplay` | `components/settings/RecoveryCodeDisplay.vue` |
| `TwoFactorSettings` | `components/settings/TwoFactorSettings.vue` |

---

## 13. 未決事項

| 項目 | 内容 |
|------|------|
| SMS OTP | セキュリティが TOTP より低く、SIM スワップ攻撃のリスクあり。現仕様では非対応。要検討 |
| メール OTP | パスワードリセットフローに組み込むかどうか |
| テナント強制の猶予期間 | 強制有効化から何日以内に設定しなければアクセス不可にするか（例: 7 日間の猶予） |
