//! アプリ設定から GitHub App クライアントを組み立てる。

use common::settings::GithubAppSettings;
use forge_github::{GithubApp, GithubAppCredentials, GithubAppOAuthCredentials};
use reqwest::Client;

/// GitHub API に送る User-Agent。
const USER_AGENT: &str = "task-backend";

/// GitHub のベース URL を差し替えてよい宛先。
///
/// ここを差し替えると GitHub の資格情報を載せたリクエストの宛先が変わるので、
/// 統合テストのモックサーバー（ループバック）以外は受け付けない。
/// env を 1 本間違えただけでシークレットが外へ出るのを防ぐ。
/// IPv6 は `Url::host_str` が角括弧付きで返すため、両方の表記を持つ。
const LOOPBACK_HOSTS: [&str; 4] = ["127.0.0.1", "localhost", "::1", "[::1]"];

fn loopback_base(base: &str) -> Option<String> {
    let base = base.trim();
    if base.is_empty() {
        return None;
    }
    let host = reqwest::Url::parse(base).ok()?.host_str()?.to_owned();
    if LOOPBACK_HOSTS.contains(&host.as_str()) {
        Some(base.to_owned())
    } else {
        None
    }
}

/// 環境変数に設定された差し替え先を、ループバック宛てのときだけ返す。
pub(super) fn loopback_base_override(var: &str) -> Option<String> {
    let base = std::env::var(var).ok()?;
    if base.trim().is_empty() {
        return None;
    }
    match loopback_base(&base) {
        Some(base) => Some(base),
        None => {
            let host = reqwest::Url::parse(base.trim())
                .ok()
                .and_then(|url| url.host_str().map(str::to_owned));
            tracing::warn!(
                variable = var,
                host = host.as_deref().unwrap_or("<invalid>"),
                "ignoring non-loopback GitHub base URL"
            );
            None
        }
    }
}

/// 設定から [`GithubApp`] を作る。
///
/// `GITHUB_API_BASE_URL` / `GITHUB_OAUTH_BASE_URL` が設定されていればベース URL を
/// 差し替える（統合テストがモックサーバーを向けるために使う）。API とユーザー認可は
/// GitHub でもホストが分かれている（`api.github.com` と `github.com`）ため、
/// 差し替え先も別々に持つ。
pub fn github_app(http: &Client, settings: &GithubAppSettings) -> GithubApp {
    let app = GithubApp::new(
        http.clone(),
        GithubAppCredentials::new(
            settings.github_app_id.clone(),
            settings.github_app_private_key.clone(),
        ),
    )
    .with_user_agent(USER_AGENT)
    .with_oauth_credentials(GithubAppOAuthCredentials::new(
        settings.github_app_client_id.clone(),
        settings.github_app_client_secret.clone(),
    ));

    let app = match loopback_base_override("GITHUB_API_BASE_URL") {
        Some(base) => app.with_api_base(base),
        None => app,
    };
    match loopback_base_override("GITHUB_OAUTH_BASE_URL") {
        Some(base) => app.with_oauth_base(base),
        None => app,
    }
}

#[cfg(test)]
mod tests {
    use super::loopback_base;

    #[test]
    fn accepts_loopback_github_base_urls() {
        for base in [
            "http://127.0.0.1:8080",
            "http://localhost:8080/",
            "http://[::1]:8080",
        ] {
            assert_eq!(loopback_base(base).as_deref(), Some(base));
        }
    }

    #[test]
    fn rejects_non_loopback_github_base_urls() {
        for base in [
            "https://example.com",
            "http://192.0.2.1:8080",
            "not a url",
            "",
            "   ",
        ] {
            assert_eq!(loopback_base(base), None, "base URL: {base:?}");
        }
    }
}
