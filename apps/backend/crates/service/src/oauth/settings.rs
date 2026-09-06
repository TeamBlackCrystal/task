//! OAuth 設定（環境変数から読み込み）。

use auth_core::provider::ProviderConfig;

#[derive(Clone, Debug)]
pub struct OAuthSettings {
    /// OAuth コールバック URL のベース（例: `http://localhost:3400`）
    pub app_base_url: String,
    /// トークン暗号化の鍵材料。実際の鍵は HKDF で導出される
    pub encryption_key: String,
    /// ログイン後のデフォルトリダイレクト先（フロント相対パス）
    pub default_redirect_path: String,
    pub github: Option<ProviderConfig>,
    pub gitlab: Option<ProviderConfig>,
    pub gitlab_selfhosted: Option<ProviderConfig>,
    pub google: Option<ProviderConfig>,
    pub oidc: Option<OidcConfig>,
}

#[derive(Clone, Debug)]
pub struct OidcConfig {
    pub issuer_url: String,
    pub client_id: String,
    pub client_secret: String,
}

impl OAuthSettings {
    pub fn from_env() -> Result<Self, anyhow::Error> {
        dotenvy::dotenv().ok();

        let app_base_url =
            env_var("APP_BASE_URL").unwrap_or_else(|| "http://localhost:3400".to_string());

        let github = pair_config(
            env_var("OAUTH_GITHUB_CLIENT_ID"),
            env_var("OAUTH_GITHUB_CLIENT_SECRET"),
        );
        let gitlab = pair_config(
            env_var("OAUTH_GITLAB_CLIENT_ID"),
            env_var("OAUTH_GITLAB_CLIENT_SECRET"),
        );
        let gitlab_selfhosted = pair_config(
            env_var("OAUTH_GITLAB_SELFHOSTED_CLIENT_ID"),
            env_var("OAUTH_GITLAB_SELFHOSTED_CLIENT_SECRET"),
        );
        let google = pair_config(
            env_var("OAUTH_GOOGLE_CLIENT_ID"),
            env_var("OAUTH_GOOGLE_CLIENT_SECRET"),
        );
        let oidc = match (
            env_var("OAUTH_OIDC_ISSUER_URL"),
            env_var("OAUTH_OIDC_CLIENT_ID"),
            env_var("OAUTH_OIDC_CLIENT_SECRET"),
        ) {
            (Some(issuer_url), Some(client_id), Some(client_secret)) => Some(OidcConfig {
                issuer_url: issuer_url.trim_end_matches('/').to_string(),
                client_id,
                client_secret,
            }),
            _ => None,
        };

        let has_providers = github.is_some()
            || gitlab.is_some()
            || gitlab_selfhosted.is_some()
            || google.is_some()
            || oidc.is_some();

        let encryption_key =
            parse_encryption_key(env_var("OAUTH_ENCRYPTION_KEY").as_deref(), has_providers)?;

        let default_redirect_path =
            env_var("OAUTH_DEFAULT_REDIRECT_PATH").unwrap_or_else(|| "/dashboard".to_string());

        Ok(Self {
            app_base_url: app_base_url.trim_end_matches('/').to_string(),
            encryption_key,
            default_redirect_path,
            github,
            gitlab,
            gitlab_selfhosted,
            google,
            oidc,
        })
    }

    pub fn callback_url(&self, provider_slug: &str) -> String {
        format!(
            "{}/v1/auth/oauth/{provider_slug}/callback",
            self.app_base_url
        )
    }

    pub fn is_provider_configured(&self, provider_slug: &str) -> bool {
        match provider_slug {
            "github" => self.github.is_some(),
            "gitlab" => self.gitlab.is_some(),
            "gitlab_selfhosted" => self.gitlab_selfhosted.is_some(),
            "google" => self.google.is_some(),
            "oidc" => self.oidc.is_some(),
            _ => false,
        }
    }

    pub fn has_any_provider(&self) -> bool {
        self.github.is_some()
            || self.gitlab.is_some()
            || self.gitlab_selfhosted.is_some()
            || self.google.is_some()
            || self.oidc.is_some()
    }

    /// DB に保存する provider キー（汎用 OIDC は `oidc:{issuer}`）。
    pub fn db_provider_key(&self, provider_slug: &str) -> Option<String> {
        match provider_slug {
            "github" | "gitlab" | "gitlab_selfhosted" | "google" => Some(provider_slug.to_string()),
            "oidc" => self.oidc.as_ref().map(|c| format!("oidc:{}", c.issuer_url)),
            _ => None,
        }
    }
}

fn env_var(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn pair_config(client_id: Option<String>, client_secret: Option<String>) -> Option<ProviderConfig> {
    match (client_id, client_secret) {
        (Some(id), Some(secret)) => Some(ProviderConfig {
            client_id: id,
            client_secret: secret,
        }),
        _ => None,
    }
}

/// 鍵材料をそのまま保持する（鍵の導出は `auth_core::crypto` 側の HKDF が行う）。
fn parse_encryption_key(raw: Option<&str>, require: bool) -> Result<String, anyhow::Error> {
    let Some(key_str) = raw.filter(|s| !s.is_empty()) else {
        if require {
            anyhow::bail!(
                "OAUTH_ENCRYPTION_KEY is required when at least one OAuth provider is configured"
            );
        }
        return Ok(String::new());
    };

    if key_str.len() < 32 {
        anyhow::bail!("OAUTH_ENCRYPTION_KEY must be at least 32 characters");
    }

    Ok(key_str.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encryption_key_optional_without_providers() {
        assert_eq!(parse_encryption_key(None, false).unwrap(), "");
    }

    #[test]
    fn encryption_key_required_with_providers() {
        assert!(parse_encryption_key(None, true).is_err());
    }

    #[test]
    fn encryption_key_rejects_short_value() {
        assert!(parse_encryption_key(Some(&"a".repeat(31)), true).is_err());
    }

    /// 連携一覧が返す識別子。OIDC だけ開始用 slug と形が違うので、画面はこちらで突き合わせる。
    #[test]
    fn db_provider_key_carries_oidc_issuer() {
        let mut settings = OAuthSettings {
            app_base_url: "http://localhost:3400".to_string(),
            encryption_key: "k".repeat(32),
            default_redirect_path: "/dashboard".to_string(),
            github: None,
            gitlab: None,
            gitlab_selfhosted: None,
            google: None,
            oidc: Some(OidcConfig {
                issuer_url: "https://idp.example.com".to_string(),
                client_id: "id".to_string(),
                client_secret: "secret".to_string(),
            }),
        };
        assert_eq!(
            settings.db_provider_key("oidc").as_deref(),
            Some("oidc:https://idp.example.com")
        );
        assert_eq!(
            settings.db_provider_key("github").as_deref(),
            Some("github")
        );

        settings.oidc = None;
        assert_eq!(settings.db_provider_key("oidc"), None);
    }

    #[test]
    fn encryption_key_keeps_full_material() {
        // 先頭 32 バイトへの切り詰めをしない（鍵材料全体が HKDF に入る）。
        let long = "a".repeat(64);
        assert_eq!(parse_encryption_key(Some(&long), true).unwrap(), long);
    }
}
