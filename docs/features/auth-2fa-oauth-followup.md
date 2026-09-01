---
title: OAuth ログイン × 2FA 統合（フォローアップ）
description: feat/auth-oauth マージ後に oauth_callback へ half_authed を適用する
---

# OAuth ログイン × 2FA（フォローアップ）

> **ステータス**: Open / PR #45 では `establish_login_session` を OAuth から呼べるようエクスポート済み

## 背景

`feat/auth-2fa` ブランチには OAuth ハンドラが未マージのため、`oauth_callback` への `half_authed` 適用は本 PR では未実装。

`apps/backend/src/handlers/auth_2fa.rs` の `establish_login_session` を、
`feat/auth-oauth` マージ後の `oauth_callback`（`session.set("user_id", …)` の直後）から呼び出す。

## 受け入れ条件

1. 2FA 有効ユーザーが OAuth ログインした場合、`half_authed: true` セッションになる
2. フロントは `/auth/2fa` へリダイレクト（クエリ `requires_2fa=true` 等は既存ログイン API と揃える）
3. `POST /v1/auth/2fa/verify` で完全認証に昇格
4. 統合テスト: OAuth モックまたは E2E で half_authed → verify フローを検証

## 関連

- [2FA 仕様書](/features/auth-2fa) §5
- [OAuth 仕様書](/features/auth-oauth)
