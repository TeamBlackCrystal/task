//! GitHub Issues REST 呼び出し（インポートと書き戻しで使う分だけ）。
//!
//! `forge-github` は GitHub App のインストール周りを扱うクレートで、Issue API は持たない。
//! Issue はタスク同期というこのアプリ固有の用途にしか使わないため、こちら側に置いている。

use reqwest::{Client, Method};
use serde::Deserialize;

use super::sync::SyncedContent;

const USER_AGENT: &str = "task-backend";
const API_VERSION: &str = "2022-11-28";
const DEFAULT_API_BASE: &str = "https://api.github.com";

/// 1 ページあたりの取得件数（GitHub の上限）。
pub const PER_PAGE: u32 = 100;

#[derive(Debug, Clone, Deserialize)]
pub struct GithubIssue {
    pub number: i32,
    pub title: String,
    pub body: Option<String>,
    /// `open` / `closed`
    pub state: String,
    /// `/issues` は PR も返すため、このフィールドの有無で PR を判別する。
    #[serde(default)]
    pub pull_request: Option<serde_json::Value>,
}

impl GithubIssue {
    pub fn is_pull_request(&self) -> bool {
        self.pull_request.is_some()
    }

    pub fn is_closed(&self) -> bool {
        self.state == "closed"
    }
}

/// `GITHUB_API_BASE_URL` が設定されていればそれを使う（統合テストがモックサーバーを向ける）。
fn api_base() -> String {
    match std::env::var("GITHUB_API_BASE_URL") {
        Ok(base) if !base.trim().is_empty() => base.trim_end_matches('/').to_string(),
        _ => DEFAULT_API_BASE.to_string(),
    }
}

fn request(http: &Client, method: Method, url: &str, token: &str) -> reqwest::RequestBuilder {
    http.request(method, url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", API_VERSION)
        .header("User-Agent", USER_AGENT)
}

/// Issue を 1 ページ分取得する。PR も混ざったまま返す
/// （ページングの終端判定は生の件数で行う必要があるため、除外は呼び出し側）。
pub async fn list_issues(
    http: &Client,
    token: &str,
    owner: &str,
    repo: &str,
    page: u32,
) -> Result<Vec<GithubIssue>, anyhow::Error> {
    let url = format!(
        "{}/repos/{owner}/{repo}/issues?state=all&per_page={PER_PAGE}&page={page}&sort=created&direction=asc",
        api_base()
    );
    let res = request(http, Method::GET, &url, token).send().await?;

    let status = res.status();
    if !status.is_success() {
        let body = res.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("list issues failed: {status}: {body}"));
    }
    Ok(res.json().await?)
}

/// Issue のタイトル・本文・開閉状態を書き戻す。
pub async fn update_issue(
    http: &Client,
    token: &str,
    owner: &str,
    repo: &str,
    number: i32,
    content: &SyncedContent,
) -> Result<(), anyhow::Error> {
    let url = format!("{}/repos/{owner}/{repo}/issues/{number}", api_base());
    let res = request(http, Method::PATCH, &url, token)
        .json(&serde_json::json!({
            "title": content.title,
            "body": content.body,
            "state": if content.closed { "closed" } else { "open" },
        }))
        .send()
        .await?;

    let status = res.status();
    if !status.is_success() {
        let body = res.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("update issue failed: {status}: {body}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(json: serde_json::Value) -> GithubIssue {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn detects_pull_request_entries() {
        let pr = issue(serde_json::json!({
            "number": 7, "title": "feat", "body": null, "state": "open",
            "pull_request": { "url": "https://api.github.com/repos/o/r/pulls/7" }
        }));
        assert!(pr.is_pull_request());

        let plain = issue(serde_json::json!({
            "number": 8, "title": "bug", "body": "detail", "state": "closed"
        }));
        assert!(!plain.is_pull_request());
        assert!(plain.is_closed());
    }
}
