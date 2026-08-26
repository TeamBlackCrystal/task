//! PR への要約コメント（GitHub Issue Comments API）。
//!
//! レビュー指摘の一覧・状態は task 側が権威で、GitHub には**マーカー付きの
//! コメント 1 本**だけを置く。2 回目以降は同じコメントを編集して更新する
//! （インライン投稿はしない。仕様 `docs/features/review-findings.md` §7）。

use reqwest::{Client, Method};
use serde::Deserialize;

const USER_AGENT: &str = "task-backend";
const API_VERSION: &str = "2022-11-28";
const DEFAULT_API_BASE: &str = "https://api.github.com";

/// 自分が書いたコメントを特定するための印。本文の先頭に置く。
pub const SUMMARY_MARKER: &str = "<!-- koyori-review-summary -->";

/// マーカー探索で読むコメントの最大ページ数（100 件 × 5 ページ）。
/// これを超えて遡らないのは、要約コメントは初回に作られるため実際には
/// 1 ページ目で見つかるのが普通で、長大な PR で全ページを舐める費用に
/// 見合わないため。見つからなければ新規投稿になる（重複は起きうるが、
/// 次回以降は新しい方が見つかって更新され続ける）。
const MAX_SEARCH_PAGES: u32 = 5;
const PER_PAGE: u32 = 100;

#[derive(Debug, Clone, Deserialize)]
struct IssueComment {
    id: i64,
    #[serde(default)]
    body: Option<String>,
}

/// PR のメタ情報のうち、表示用にキャッシュするもの。
#[derive(Debug, Clone, Deserialize)]
pub struct PullRequestMeta {
    pub title: String,
    pub user: Option<PullRequestUser>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PullRequestUser {
    pub login: String,
}

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

/// PR のタイトルと作者を取る。表示用のキャッシュにしか使わない。
pub async fn fetch_pull_request(
    http: &Client,
    token: &str,
    owner: &str,
    repo: &str,
    number: i32,
) -> Result<PullRequestMeta, anyhow::Error> {
    let url = format!("{}/repos/{owner}/{repo}/pulls/{number}", api_base());
    let res = request(http, Method::GET, &url, token).send().await?;
    let status = res.status();
    if !status.is_success() {
        let body = res.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!(
            "fetch pull request failed: {status}: {body}"
        ));
    }
    Ok(res.json::<PullRequestMeta>().await?)
}

/// マーカー付きのコメントを探す。無ければ `None`。
async fn find_summary_comment(
    http: &Client,
    token: &str,
    owner: &str,
    repo: &str,
    number: i32,
) -> Result<Option<i64>, anyhow::Error> {
    for page in 1..=MAX_SEARCH_PAGES {
        let url = format!(
            "{}/repos/{owner}/{repo}/issues/{number}/comments?per_page={PER_PAGE}&page={page}",
            api_base()
        );
        let res = request(http, Method::GET, &url, token).send().await?;
        let status = res.status();
        if !status.is_success() {
            let body = res.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("list pr comments failed: {status}: {body}"));
        }
        let comments = res.json::<Vec<IssueComment>>().await?;
        let fetched = comments.len();

        if let Some(found) = comments.into_iter().find(|c| {
            c.body
                .as_deref()
                .is_some_and(|b| b.contains(SUMMARY_MARKER))
        }) {
            return Ok(Some(found.id));
        }
        if (fetched as u32) < PER_PAGE {
            break;
        }
    }
    Ok(None)
}

/// 要約コメントを作るか、既にあれば同じコメントを更新する。
///
/// 戻り値は使ったコメント ID。呼び出し側はログにだけ使う。
pub async fn upsert_summary_comment(
    http: &Client,
    token: &str,
    owner: &str,
    repo: &str,
    number: i32,
    body: &str,
) -> Result<i64, anyhow::Error> {
    let payload = serde_json::json!({ "body": body });

    if let Some(comment_id) = find_summary_comment(http, token, owner, repo, number).await? {
        let url = format!(
            "{}/repos/{owner}/{repo}/issues/comments/{comment_id}",
            api_base()
        );
        let res = request(http, Method::PATCH, &url, token)
            .json(&payload)
            .send()
            .await?;
        let status = res.status();
        if !status.is_success() {
            let text = res.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "update pr comment failed: {status}: {text}"
            ));
        }
        return Ok(comment_id);
    }

    let url = format!(
        "{}/repos/{owner}/{repo}/issues/{number}/comments",
        api_base()
    );
    let res = request(http, Method::POST, &url, token)
        .json(&payload)
        .send()
        .await?;
    let status = res.status();
    if !status.is_success() {
        let text = res.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!(
            "create pr comment failed: {status}: {text}"
        ));
    }
    Ok(res.json::<IssueComment>().await?.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_is_an_html_comment_so_it_stays_invisible() {
        assert!(SUMMARY_MARKER.starts_with("<!--"));
        assert!(SUMMARY_MARKER.ends_with("-->"));
    }
}
