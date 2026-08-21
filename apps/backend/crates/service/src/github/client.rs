//! アプリ設定から GitHub App クライアントを組み立てる。

use common::settings::GithubAppSettings;
use forge_github::{GithubApp, GithubAppCredentials, GithubAppOAuthCredentials};
use reqwest::Client;

/// GitHub API に送る User-Agent。
const USER_AGENT: &str = "task-backend";

/// OAuth のベース URL を差し替えてよい宛先。
///
/// ここを差し替えると Client secret を載せた POST の宛先が変わるので、
/// 統合テストのモックサーバー（ループバック）以外は受け付けない。
/// env を 1 本間違えただけでシークレットが外へ出るのを防ぐ。
/// IPv6 は `Url::host_str` が角括弧付きで返すため、両方の表記を持つ。
const LOOPBACK_HOSTS: [&str; 4] = ["127.0.0.1", "localhost", "::1", "[::1]"];

/// `GITHUB_OAUTH_BASE_URL` の差し替え先。ループバック宛てのときだけ返す。
fn oauth_base_override() -> Option<String> {
    let base = std::env::var("GITHUB_OAUTH_BASE_URL").ok()?;
    let base = base.trim();
    if base.is_empty() {
        return None;
    }
    let host = reqwest::Url::parse(base).ok()?.host_str()?.to_owned();
    if LOOPBACK_HOSTS.contains(&host.as_str()) {
        Some(base.to_owned())
    } else {
        tracing::warn!(host = %host, "ignoring non-loopback GITHUB_OAUTH_BASE_URL");
        None
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

    let app = match std::env::var("GITHUB_API_BASE_URL") {
        Ok(base) if !base.trim().is_empty() => app.with_api_base(base),
        _ => app,
    };
    match oauth_base_override() {
        Some(base) => app.with_oauth_base(base),
        None => app,
    }
}
