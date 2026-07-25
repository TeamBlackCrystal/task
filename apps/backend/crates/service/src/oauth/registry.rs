//! provider slug から [`OAuthProvider`] 実装を解決する。
//!
//! GitHub / GitLab は専用クレートの実装を使う。Google と汎用 OIDC は
//! `auth_core` の OIDC 機構（discovery + 標準 userinfo）に薄くかぶせるだけなので
//! ここに置く。

use anyhow::Error;
use async_trait::async_trait;
use auth_core::client::fetch_oidc_user;
use auth_core::provider::{
    OAuthProvider, ProviderConfig, ProviderEndpoints, ProviderUserInfo, fetch_oidc_discovery,
};
use auth_core_github::GithubProvider;
use auth_core_gitlab::{GitlabProvider, GitlabSelfHostedProvider};
use reqwest::Client;

use super::settings::OAuthSettings;

/// GitHub API に送る User-Agent。
const GITHUB_USER_AGENT: &str = "task-oauth-backend";

/// Google の OAuth ログインプロバイダー（エンドポイントは固定、userinfo は OIDC 標準）。
struct GoogleProvider;

#[async_trait]
impl OAuthProvider for GoogleProvider {
    fn slug(&self) -> &str {
        "google"
    }

    async fn endpoints(&self, _http: &Client) -> Result<ProviderEndpoints, Error> {
        Ok(ProviderEndpoints {
            authorize_url: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
            token_url: "https://oauth2.googleapis.com/token".to_string(),
            userinfo_url: "https://openidconnect.googleapis.com/v1/userinfo".to_string(),
            scopes: vec!["openid", "email", "profile"],
            use_oidc_id_token: true,
        })
    }

    async fn fetch_user_info(
        &self,
        http: &Client,
        endpoints: &ProviderEndpoints,
        access_token: &str,
    ) -> Result<ProviderUserInfo, Error> {
        fetch_oidc_user(http, &endpoints.userinfo_url, access_token).await
    }
}

/// 汎用 OIDC プロバイダー。エンドポイントは discovery で解決する。
struct OidcProvider {
    issuer_url: String,
}

#[async_trait]
impl OAuthProvider for OidcProvider {
    fn slug(&self) -> &str {
        "oidc"
    }

    async fn endpoints(&self, http: &Client) -> Result<ProviderEndpoints, Error> {
        // issuer は運用者が設定する値だが、内部ネットワークを走査させないため検証する。
        auth_core::url_guard::validate_instance_url(&self.issuer_url)?;
        fetch_oidc_discovery(http, &self.issuer_url).await
    }

    async fn fetch_user_info(
        &self,
        http: &Client,
        endpoints: &ProviderEndpoints,
        access_token: &str,
    ) -> Result<ProviderUserInfo, Error> {
        fetch_oidc_user(http, &endpoints.userinfo_url, access_token).await
    }
}

/// provider slug に対応する実装を返す。
///
/// `instance_url` は `gitlab_selfhosted` のみで必要（他のプロバイダーでは無視される）。
pub fn resolve_provider(
    provider_slug: &str,
    settings: &OAuthSettings,
    instance_url: Option<&str>,
) -> Result<Box<dyn OAuthProvider>, Error> {
    match provider_slug {
        "github" => Ok(Box::new(GithubProvider::new(GITHUB_USER_AGENT))),
        "gitlab" => Ok(Box::new(GitlabProvider)),
        "gitlab_selfhosted" => {
            let base = instance_url
                .ok_or_else(|| anyhow::anyhow!("instance_url is required for gitlab_selfhosted"))?;
            Ok(Box::new(GitlabSelfHostedProvider::new(base)?))
        }
        "google" => Ok(Box::new(GoogleProvider)),
        "oidc" => {
            let issuer_url = settings
                .oidc
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("oidc provider not configured"))?
                .issuer_url
                .clone();
            Ok(Box::new(OidcProvider { issuer_url }))
        }
        other => anyhow::bail!("unsupported oauth provider: {other}"),
    }
}

pub fn get_credentials(
    provider_slug: &str,
    settings: &OAuthSettings,
) -> Result<ProviderConfig, Error> {
    match provider_slug {
        "github" => settings
            .github
            .clone()
            .ok_or_else(|| anyhow::anyhow!("github provider not configured")),
        "gitlab" => settings
            .gitlab
            .clone()
            .ok_or_else(|| anyhow::anyhow!("gitlab provider not configured")),
        "gitlab_selfhosted" => settings
            .gitlab_selfhosted
            .clone()
            .ok_or_else(|| anyhow::anyhow!("gitlab_selfhosted provider not configured")),
        "google" => settings
            .google
            .clone()
            .ok_or_else(|| anyhow::anyhow!("google provider not configured")),
        "oidc" => settings
            .oidc
            .as_ref()
            .map(|o| ProviderConfig {
                client_id: o.client_id.clone(),
                client_secret: o.client_secret.clone(),
            })
            .ok_or_else(|| anyhow::anyhow!("oidc provider not configured")),
        other => anyhow::bail!("unsupported oauth provider: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> OAuthSettings {
        OAuthSettings {
            app_base_url: "http://localhost:3400".into(),
            encryption_key: "k".repeat(32),
            default_redirect_path: "/dashboard".into(),
            github: None,
            gitlab: None,
            gitlab_selfhosted: None,
            google: None,
            oidc: None,
        }
    }

    #[test]
    fn resolves_known_providers() {
        let s = settings();
        assert_eq!(
            resolve_provider("github", &s, None).unwrap().slug(),
            "github"
        );
        assert_eq!(
            resolve_provider("gitlab", &s, None).unwrap().slug(),
            "gitlab"
        );
        assert_eq!(
            resolve_provider("google", &s, None).unwrap().slug(),
            "google"
        );
    }

    #[test]
    fn rejects_unknown_provider() {
        assert!(resolve_provider("bitbucket", &settings(), None).is_err());
    }

    #[test]
    fn self_hosted_requires_instance_url() {
        assert!(resolve_provider("gitlab_selfhosted", &settings(), None).is_err());
    }

    #[test]
    fn self_hosted_rejects_private_instance_url() {
        // SSRF: 内部ネットワークへ向いた instance_url は解決段階で弾く。
        assert!(
            resolve_provider("gitlab_selfhosted", &settings(), Some("https://10.0.0.5")).is_err()
        );
    }

    #[test]
    fn oidc_requires_configuration() {
        assert!(resolve_provider("oidc", &settings(), None).is_err());
    }
}
