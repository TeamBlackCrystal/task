//! アプリ設定から GitHub App クライアントを組み立てる。

use common::settings::GithubAppSettings;
use forge_github::{GithubApp, GithubAppCredentials};
use reqwest::Client;

/// GitHub API に送る User-Agent。
const USER_AGENT: &str = "task-backend";

/// 設定から [`GithubApp`] を作る。
///
/// `GITHUB_API_BASE_URL` が設定されていればベース URL を差し替える
/// （統合テストがモックサーバーを向けるために使う）。
pub fn github_app(http: &Client, settings: &GithubAppSettings) -> GithubApp {
    let app = GithubApp::new(
        http.clone(),
        GithubAppCredentials::new(
            settings.github_app_id.clone(),
            settings.github_app_private_key.clone(),
        ),
    )
    .with_user_agent(USER_AGENT);

    match std::env::var("GITHUB_API_BASE_URL") {
        Ok(base) if !base.trim().is_empty() => app.with_api_base(base),
        _ => app,
    }
}
