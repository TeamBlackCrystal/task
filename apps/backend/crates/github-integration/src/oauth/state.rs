//! OAuth state を Redis に保存・検証する。

use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;
use uuid::Uuid;

use super::config::OAuthSettings;
use common::cache::redis::RedisConnection;

/// OAuth state の TTL（秒）。仕様: 10 分。
pub const STATE_TTL_SECS: u64 = 10 * 60;

const KEY_PREFIX: &str = "oauth:state:";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthStatePayload {
    /// OAuth フロー開始時のプロバイダー slug（callback で照合して CSRF を防ぐ）
    pub provider: String,
    pub code_verifier: String,
    pub redirect_after: String,
    /// プロバイダーがエラーを返したときの戻り先。成功用 `redirect_after` とは別に保持し、
    /// OAuth ボタンを描画するページ（signin/signup）へ戻してエラーを表示するために使う。
    pub error_redirect_after: String,
    /// アカウント連携時のログイン済みユーザー ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_user_id: Option<Uuid>,
    /// GitLab self-hosted のインスタンス URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_url: Option<String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RedirectValidationError {
    #[error("redirect path must be a relative path starting with /")]
    NotRelative,
    #[error("redirect path contains disallowed characters or patterns")]
    DisallowedPattern,
    #[error("redirect path must stay on the configured frontend origin")]
    OriginMismatch,
    #[error("invalid frontend base URL")]
    InvalidBase,
}

pub async fn store_state(
    redis: &RedisConnection,
    state: &str,
    payload: &OAuthStatePayload,
) -> Result<(), anyhow::Error> {
    let mut conn = redis
        .conn
        .acquire()
        .await
        .map_err(|e| anyhow::anyhow!("redis acquire: {e}"))?;

    let key = format!("{KEY_PREFIX}{state}");
    let value = serde_json::to_string(payload)?;
    let _: () = redis::cmd("SET")
        .arg(&key)
        .arg(value)
        .arg("EX")
        .arg(STATE_TTL_SECS)
        .query_async(&mut conn)
        .await
        .map_err(|e| anyhow::anyhow!("redis SET oauth state: {e}"))?;

    Ok(())
}

/// state を検証して取得し、Redis から即削除する（使い捨て）。
pub async fn consume_state(
    redis: &RedisConnection,
    state: &str,
) -> Result<Option<OAuthStatePayload>, anyhow::Error> {
    let mut conn = redis
        .conn
        .acquire()
        .await
        .map_err(|e| anyhow::anyhow!("redis acquire: {e}"))?;

    let key = format!("{KEY_PREFIX}{state}");
    let value: Option<String> = redis::cmd("GETDEL")
        .arg(&key)
        .query_async(&mut conn)
        .await
        .map_err(|e| anyhow::anyhow!("redis GETDEL oauth state: {e}"))?;

    let Some(raw) = value else {
        return Ok(None);
    };

    let payload: OAuthStatePayload = serde_json::from_str(&raw)?;
    Ok(Some(payload))
}

/// `redirect_after` を相対パスとして検証・正規化する（オープンリダイレクト対策）。
pub fn sanitize_redirect_path(path: &str) -> Result<String, RedirectValidationError> {
    let path = path.trim();
    if !path.starts_with('/') {
        return Err(RedirectValidationError::NotRelative);
    }
    if path.starts_with("//")
        || path.contains("://")
        || path.contains(':')
        || path.contains('\\')
        || path.contains('@')
        || path.contains("..")
    {
        return Err(RedirectValidationError::DisallowedPattern);
    }
    Ok(path.to_string())
}

/// フロントへのリダイレクト URL を組み立てる（同一 origin のみ許可）。
pub fn build_frontend_redirect(
    frontend_base: &str,
    redirect_after: &str,
    _settings: &OAuthSettings,
) -> Result<String, RedirectValidationError> {
    let path = sanitize_redirect_path(redirect_after)?;
    let base = Url::parse(frontend_base.trim_end_matches('/'))
        .map_err(|_| RedirectValidationError::InvalidBase)?;

    let joined = base
        .join(path.trim_start_matches('/'))
        .map_err(|_| RedirectValidationError::InvalidBase)?;

    if joined.origin() != base.origin() {
        return Err(RedirectValidationError::OriginMismatch);
    }

    Ok(joined.to_string())
}

/// OAuth プロバイダーが error を返した場合のフロントリダイレクト URL。
pub fn build_frontend_oauth_error_redirect(
    frontend_base: &str,
    redirect_after: &str,
    settings: &OAuthSettings,
) -> Result<String, RedirectValidationError> {
    let base_url = build_frontend_redirect(frontend_base, redirect_after, settings)?;
    let mut url = Url::parse(&base_url).map_err(|_| RedirectValidationError::InvalidBase)?;
    url.query_pairs_mut()
        .append_pair("oauth_error", "authorization_failed");
    Ok(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oauth::OAuthSettings;

    fn test_settings() -> OAuthSettings {
        OAuthSettings {
            app_base_url: "http://localhost:3400".into(),
            encryption_key: [0u8; 32],
            default_redirect_path: "/dashboard".into(),
            github: None,
            gitlab: None,
            gitlab_selfhosted: None,
            google: None,
            oidc: None,
        }
    }

    #[test]
    fn sanitize_rejects_protocol_relative() {
        assert_eq!(
            sanitize_redirect_path("//evil.com"),
            Err(RedirectValidationError::DisallowedPattern)
        );
    }

    #[test]
    fn sanitize_rejects_absolute_url() {
        assert_eq!(
            sanitize_redirect_path("https://evil.com"),
            Err(RedirectValidationError::NotRelative)
        );
    }

    #[test]
    fn sanitize_rejects_colon_and_at() {
        assert!(sanitize_redirect_path("/foo:bar").is_err());
        assert!(sanitize_redirect_path("/user@host").is_err());
    }

    #[test]
    fn sanitize_accepts_safe_relative_path() {
        assert_eq!(
            sanitize_redirect_path("/dashboard"),
            Ok("/dashboard".to_string())
        );
    }

    #[test]
    fn build_redirect_stays_on_frontend_origin() {
        let settings = test_settings();
        let url =
            build_frontend_redirect("https://app.example.com", "/settings/profile", &settings)
                .unwrap();
        assert_eq!(url, "https://app.example.com/settings/profile");
    }

    #[test]
    fn build_redirect_rejects_open_redirect_via_path() {
        let settings = test_settings();
        assert!(
            build_frontend_redirect("https://app.example.com", "//evil.com/phish", &settings,)
                .is_err()
        );
    }

    #[test]
    fn oauth_error_redirect_includes_query_param() {
        let settings = test_settings();
        let url =
            build_frontend_oauth_error_redirect("https://app.example.com", "/login", &settings)
                .unwrap();
        assert!(url.contains("oauth_error=authorization_failed"));
        assert!(url.starts_with("https://app.example.com/login"));
    }
}
