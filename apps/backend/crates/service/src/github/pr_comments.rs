//! PR への要約コメント（GitHub Issue Comments API）。
//!
//! レビュー指摘の一覧・状態は task 側が権威で、GitHub には**マーカー付きの
//! コメント 1 本**だけを置く。2 回目以降は同じコメントを編集して更新する
//! （インライン投稿はしない。仕様 `docs/features/review-findings.md` §7）。

use reqwest::{Client, Method, StatusCode};
use serde::Deserialize;
use uuid::Uuid;

use super::client::api_base;

const USER_AGENT: &str = "task-backend";
const API_VERSION: &str = "2022-11-28";

/// 自分が書いたコメントを特定するための印。本文の先頭に置く。
///
/// プロジェクト ID を含めるのは、同じリポジトリへ 2 つのプロジェクトが連携したときに
/// 互いのコメントを自分のものと誤認して交互に上書きするのを防ぐため（仕様 §7）。
#[must_use]
pub fn summary_marker(project_id: Uuid) -> String {
    format!("<!-- koyori-review-summary:{project_id} -->")
}

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
    #[serde(default)]
    user: Option<CommentUser>,
}

#[derive(Debug, Clone, Deserialize)]
struct CommentUser {
    #[serde(default)]
    login: Option<String>,
    #[serde(rename = "type", default)]
    kind: Option<String>,
}

impl IssueComment {
    /// このコメントが自分（GitHub App の bot）のものか。
    ///
    /// マーカーは PR の参加者なら誰でも本文に書けるので、マーカーだけで特定すると
    /// 第三者が先取りできる——App は他人のコメントを編集できないため更新は失敗し続け、
    /// 失敗はベストエフォートで握り潰されるので正式な要約が永久に作られない（仕様 §7）。
    fn is_written_by(&self, bot_login: &str) -> bool {
        self.user.as_ref().is_some_and(|u| {
            u.kind.as_deref() == Some("Bot") && u.login.as_deref() == Some(bot_login)
        })
    }
}

/// PR のメタ情報のうち、表示用にキャッシュするもの。
#[derive(Debug, Clone, Deserialize)]
pub struct PullRequestMeta {
    pub title: String,
    pub user: Option<PullRequestUser>,
    /// 現在の PR head。レビューした commit との照合に使う（仕様 §7）
    #[serde(default)]
    pub head: Option<PullRequestHead>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PullRequestHead {
    pub sha: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PullRequestUser {
    pub login: String,
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

/// 自分が書いたマーカー付きのコメントを探す。無ければ `None`。
///
/// 条件は「マーカー一致」かつ「作成者が自分の bot」の両方。第三者が同じマーカーを
/// 書いたコメントは無視する（無視しないと、編集できないコメントを掴んで更新が
/// 失敗し続ける。仕様 §7）。
async fn find_summary_comment(
    http: &Client,
    token: &str,
    owner: &str,
    repo: &str,
    number: i32,
    marker: &str,
    bot_login: &str,
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
            c.body.as_deref().is_some_and(|b| b.contains(marker)) && c.is_written_by(bot_login)
        }) {
            return Ok(Some(found.id));
        }
        if (fetched as u32) < PER_PAGE {
            break;
        }
    }
    Ok(None)
}

/// 要約コメントの投稿先と本文。
pub struct SummaryCommentTarget<'a> {
    pub owner: &'a str,
    pub repo: &'a str,
    pub number: i32,
    /// 自分のコメントを見分ける印（[`summary_marker`]）
    pub marker: &'a str,
    /// 自分の GitHub App の bot login（`{app-name}[bot]`）
    pub bot_login: &'a str,
    /// 前回投稿したコメントの控え。あれば探索を飛ばす
    pub known_comment_id: Option<i64>,
    pub body: &'a str,
}

/// 要約コメントを作るか、既にあれば同じコメントを更新する。
///
/// `known_comment_id` は前回投稿したコメントの控え。あれば探索せずに直接更新する。
/// 更新が **404（コメントが存在しない）を返したときだけ**控えを捨てて作り直す——
/// レート制限や 5xx でも作り直すと、一時障害のたびにコメントが増え、古い方が
/// 「マージ可」と書かれたまま PR に残る（仕様 §7）。
///
/// 戻り値は使ったコメント ID。呼び出し側は控えとして保存する。
pub async fn upsert_summary_comment(
    http: &Client,
    token: &str,
    target: &SummaryCommentTarget<'_>,
) -> Result<i64, anyhow::Error> {
    let SummaryCommentTarget {
        owner,
        repo,
        number,
        marker,
        bot_login,
        known_comment_id,
        body,
    } = *target;
    let payload = serde_json::json!({ "body": body });

    // 控えがあれば探索を飛ばす。無ければ（初回・控えを失ったとき）探す
    let existing = match known_comment_id {
        Some(id) => Some(id),
        None => find_summary_comment(http, token, owner, repo, number, marker, bot_login).await?,
    };

    if let Some(comment_id) = existing {
        let url = format!(
            "{}/repos/{owner}/{repo}/issues/comments/{comment_id}",
            api_base()
        );
        let res = request(http, Method::PATCH, &url, token)
            .json(&payload)
            .send()
            .await?;
        let status = res.status();
        if status.is_success() {
            return Ok(comment_id);
        }
        if status != StatusCode::NOT_FOUND {
            let text = res.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "update pr comment failed: {status}: {text}"
            ));
        }
        // 404 のときだけ「手で消された」と見なして作り直す
        tracing::info!(
            comment_id,
            pr = number,
            "summary comment is gone; creating a new one"
        );
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
        let marker = summary_marker(Uuid::new_v4());
        assert!(marker.starts_with("<!--"));
        assert!(marker.ends_with("-->"));
    }

    /// マーカーはプロジェクトごとに違う（同じリポジトリを見る 2 プロジェクトが
    /// 互いのコメントを自分のものと誤認しない）。
    #[test]
    fn markers_differ_per_project() {
        assert_ne!(
            summary_marker(Uuid::new_v4()),
            summary_marker(Uuid::new_v4())
        );
    }

    /// マーカーが一致しても、書いたのが自分の bot でなければ自分のコメントではない。
    /// マーカーは PR の参加者なら誰でも本文に書ける。
    #[test]
    fn a_third_party_comment_is_not_ours() {
        let ours = IssueComment {
            id: 1,
            body: Some("marker".into()),
            user: Some(CommentUser {
                login: Some("koyori-task[bot]".into()),
                kind: Some("Bot".into()),
            }),
        };
        assert!(ours.is_written_by("koyori-task[bot]"));

        // 人間が同じマーカーを書いただけ
        let impostor = IssueComment {
            id: 2,
            body: Some("marker".into()),
            user: Some(CommentUser {
                login: Some("koyori-task[bot]".into()),
                kind: Some("User".into()),
            }),
        };
        assert!(!impostor.is_written_by("koyori-task[bot]"));

        // 別の App の bot
        let other_bot = IssueComment {
            id: 3,
            body: Some("marker".into()),
            user: Some(CommentUser {
                login: Some("other[bot]".into()),
                kind: Some("Bot".into()),
            }),
        };
        assert!(!other_bot.is_written_by("koyori-task[bot]"));

        // 作成者が分からないコメント
        let unknown = IssueComment {
            id: 4,
            body: Some("marker".into()),
            user: None,
        };
        assert!(!unknown.is_written_by("koyori-task[bot]"));
    }
}
