//! GitHub Issue ↔ タスクの双方向同期。
//!
//! 同期するのは「タイトル・本文・開閉状態」の 3 つだけ。両方向で同じ 3 つ組から
//! ハッシュを取り、直前に同期した内容と一致するなら書き込みを止める。これで
//! 「書き戻し → GitHub の webhook → 取り込み → 書き戻し …」のループが止まる。

use reqwest::Client;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseConnection,
    EntityTrait, QueryFilter, QueryOrder, TransactionTrait, prelude::Uuid,
};
use sha2::{Digest, Sha256};

use common::settings::GithubAppSettings;
use entity::{github_integrations, github_issue_links, project_statuses, tasks};

use super::client::github_app;
use super::issues::{self, GithubIssue, PER_PAGE};

/// GitHub と タスクの間で突き合わせる内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncedContent {
    pub title: String,
    pub body: String,
    pub closed: bool,
}

impl SyncedContent {
    pub fn from_issue(issue: &GithubIssue) -> Self {
        Self {
            title: issue.title.clone(),
            body: issue.body.clone().unwrap_or_default(),
            closed: issue.is_closed(),
        }
    }

    pub fn from_task(task: &tasks::Model, is_done_state: bool) -> Self {
        Self {
            title: task.title.clone(),
            body: task.description.clone().unwrap_or_default(),
            closed: is_done_state,
        }
    }

    /// 同期済み判定に使うハッシュ。両方向で同じ値になるよう、正規化した 3 つ組から取る。
    pub fn hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.title.as_bytes());
        hasher.update([0]);
        hasher.update(self.body.as_bytes());
        hasher.update([0]);
        hasher.update([u8::from(self.closed)]);
        hex::encode(hasher.finalize())
    }
}

/// 空本文はタスク側では NULL として持つ（GitHub の `null` と `""` を同一視する）。
fn description_of(body: &str) -> Option<String> {
    (!body.is_empty()).then(|| body.to_string())
}

/// インストールアクセストークンを取り直す。
/// DB に保存したトークンは 1 時間で失効するため、ジョブ側では常に取り直す。
async fn installation_token(
    http: &Client,
    settings: &GithubAppSettings,
    installation_id: i64,
) -> Result<String, anyhow::Error> {
    Ok(github_app(http, settings)
        .installation_access_token(installation_id)
        .await?
        .token)
}

async fn require_integration(
    db: &DatabaseConnection,
    project_id: Uuid,
) -> Result<github_integrations::Model, anyhow::Error> {
    github_integrations::Entity::find()
        .filter(github_integrations::Column::ProjectId.eq(project_id))
        .one(db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("project {project_id} has no github integration"))
}

/// Issue の開閉状態に対応するステータスを引く。
/// closed は `is_done_state`、open は既定ステータス。
async fn resolve_status<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    closed: bool,
) -> Result<project_statuses::Model, anyhow::Error> {
    let column = if closed {
        project_statuses::Column::IsDoneState
    } else {
        project_statuses::Column::IsDefault
    };
    project_statuses::Entity::find()
        .filter(project_statuses::Column::ProjectId.eq(project_id))
        .filter(column.eq(true))
        .order_by_asc(project_statuses::Column::Position)
        .one(db)
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "project {project_id} has no {} status",
                if closed { "done" } else { "default" }
            )
        })
}

/// GitHub の Issue 1 件をタスクへ反映する（リンク済みなら更新、未リンクなら作成）。
/// 直前に同期した内容と同じなら何もしない。
pub async fn apply_issue(
    db: &DatabaseConnection,
    project_id: Uuid,
    issue: &GithubIssue,
    created_by: Uuid,
) -> Result<(), anyhow::Error> {
    let content = SyncedContent::from_issue(issue);
    let hash = content.hash();

    let txn = db.begin().await?;
    let link = github_issue_links::Entity::find()
        .filter(github_issue_links::Column::ProjectId.eq(project_id))
        .filter(github_issue_links::Column::GithubNumber.eq(issue.number))
        .one(&txn)
        .await?;

    if let Some(link) = link {
        if link.synced_hash == hash {
            txn.rollback().await?;
            return Ok(());
        }
        let task = tasks::Entity::find_by_id(link.task_id)
            .one(&txn)
            .await?
            .ok_or_else(|| anyhow::anyhow!("linked task {} is gone", link.task_id))?;
        let status = resolve_status(&txn, project_id, content.closed).await?;
        // 完了済みのまま再同期されたときに完了日時を上書きしない。
        let completed_at = task.completed_at;

        let mut active: tasks::ActiveModel = task.into();
        active.title = Set(content.title.clone());
        active.description = Set(description_of(&content.body));
        active.status_id = Set(status.id);
        active.completed_at = Set(if content.closed {
            completed_at.or_else(|| Some(chrono::Utc::now().into()))
        } else {
            None
        });
        active.updated_at = Set(chrono::Utc::now().into());
        active.update(&txn).await?;

        let mut link_active: github_issue_links::ActiveModel = link.into();
        link_active.synced_hash = Set(hash);
        link_active.updated_at = Set(chrono::Utc::now().into());
        link_active.update(&txn).await?;
    } else {
        let status = resolve_status(&txn, project_id, content.closed).await?;
        let seq_id = crate::tasks::next_seq_id(&txn, project_id).await?;
        let now = chrono::Utc::now();
        let task = tasks::ActiveModel {
            id: Set(Uuid::new_v4()),
            project_id: Set(project_id),
            seq_id: Set(seq_id),
            title: Set(content.title.clone()),
            description: Set(description_of(&content.body)),
            status_id: Set(status.id),
            priority: Set(tasks::TaskPriority::Medium),
            progress_pct: Set(0),
            parent_task_id: Set(None),
            milestone_id: Set(None),
            sprint_id: Set(None),
            soft_deadline: Set(None),
            hard_deadline: Set(None),
            estimated_minutes: Set(None),
            is_archived: Set(false),
            created_by: Set(created_by),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
            completed_at: Set(content.closed.then(|| now.into())),
            deleted_at: Set(None),
        }
        .insert(&txn)
        .await?;

        github_issue_links::ActiveModel {
            id: Set(Uuid::new_v4()),
            project_id: Set(project_id),
            task_id: Set(task.id),
            github_number: Set(issue.number),
            synced_hash: Set(hash),
            updated_at: Set(now.into()),
        }
        .insert(&txn)
        .await?;
    }

    txn.commit().await?;
    Ok(())
}

/// webhook の `issues` イベントで反映する action。
/// `deleted` は含めない（消えた Issue をタスクとして復活させないため）。
const SYNCED_ACTIONS: [&str; 4] = ["opened", "edited", "closed", "reopened"];

/// webhook の `issues` イベントをタスクへ反映する。反映したら `true`。
pub async fn apply_issue_event(
    db: &DatabaseConnection,
    project_id: Uuid,
    payload: &serde_json::Value,
) -> Result<bool, anyhow::Error> {
    let action = payload.get("action").and_then(|a| a.as_str()).unwrap_or("");
    if !SYNCED_ACTIONS.contains(&action) {
        return Ok(false);
    }
    let Some(raw_issue) = payload.get("issue") else {
        return Ok(false);
    };
    let issue: GithubIssue = serde_json::from_value(raw_issue.clone())?;
    if issue.is_pull_request() {
        return Ok(false);
    }
    let integration = require_integration(db, project_id).await?;
    apply_issue(db, project_id, &issue, integration.created_by).await?;
    Ok(true)
}

/// リポジトリの Issue を全件取り込む。すでに取り込み済みのものは内容が変わっていなければ触らない。
pub async fn import_project(
    db: &DatabaseConnection,
    http: &Client,
    settings: &GithubAppSettings,
    project_id: Uuid,
) -> Result<usize, anyhow::Error> {
    let integration = require_integration(db, project_id).await?;
    let token = installation_token(http, settings, integration.installation_id).await?;

    let mut imported = 0usize;
    let mut page = 1u32;
    loop {
        let batch = issues::list_issues(
            http,
            &token,
            &integration.repo_owner,
            &integration.repo_name,
            page,
        )
        .await?;
        let fetched = batch.len() as u32;

        for issue in batch.iter().filter(|i| !i.is_pull_request()) {
            apply_issue(db, project_id, issue, integration.created_by).await?;
            imported += 1;
        }

        if fetched < PER_PAGE {
            break;
        }
        page += 1;
    }
    Ok(imported)
}

/// タスク側の変更を GitHub Issue へ書き戻す。リンクが無いタスクは対象外。
pub async fn push_task(
    db: &DatabaseConnection,
    http: &Client,
    settings: &GithubAppSettings,
    task_id: Uuid,
) -> Result<(), anyhow::Error> {
    let Some(link) = github_issue_links::Entity::find()
        .filter(github_issue_links::Column::TaskId.eq(task_id))
        .one(db)
        .await?
    else {
        return Ok(());
    };

    let Some(task) = tasks::Entity::find_by_id(task_id)
        .filter(tasks::Column::DeletedAt.is_null())
        .one(db)
        .await?
    else {
        return Ok(());
    };

    let status = project_statuses::Entity::find_by_id(task.status_id)
        .one(db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("task {task_id} points at a missing status"))?;

    let content = SyncedContent::from_task(&task, status.is_done_state);
    let hash = content.hash();
    if link.synced_hash == hash {
        return Ok(());
    }

    let integration = require_integration(db, link.project_id).await?;
    let token = installation_token(http, settings, integration.installation_id).await?;
    issues::update_issue(
        http,
        &token,
        &integration.repo_owner,
        &integration.repo_name,
        link.github_number,
        &content,
    )
    .await?;

    let mut active: github_issue_links::ActiveModel = link.into();
    active.synced_hash = Set(hash);
    active.updated_at = Set(chrono::Utc::now().into());
    active.update(db).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content(title: &str, body: &str, closed: bool) -> SyncedContent {
        SyncedContent {
            title: title.to_string(),
            body: body.to_string(),
            closed,
        }
    }

    #[test]
    fn same_content_from_both_sides_hashes_equal() {
        let issue: GithubIssue = serde_json::from_value(serde_json::json!({
            "number": 1, "title": "ログインが落ちる", "body": "手順:", "state": "closed"
        }))
        .unwrap();
        let from_github = SyncedContent::from_issue(&issue);
        let from_task = content("ログインが落ちる", "手順:", true);
        assert_eq!(from_github.hash(), from_task.hash());
    }

    #[test]
    fn null_body_and_empty_body_hash_equal() {
        let issue: GithubIssue = serde_json::from_value(serde_json::json!({
            "number": 1, "title": "t", "body": null, "state": "open"
        }))
        .unwrap();
        assert_eq!(
            SyncedContent::from_issue(&issue).hash(),
            content("t", "", false).hash()
        );
    }

    #[test]
    fn each_field_changes_the_hash() {
        let base = content("t", "b", false);
        assert_ne!(base.hash(), content("t2", "b", false).hash());
        assert_ne!(base.hash(), content("t", "b2", false).hash());
        assert_ne!(base.hash(), content("t", "b", true).hash());
    }

    /// タイトルと本文の境界が無いと "ab" + "" と "a" + "b" が同じハッシュになる。
    #[test]
    fn field_boundaries_are_separated() {
        assert_ne!(
            content("ab", "", false).hash(),
            content("a", "b", false).hash()
        );
    }

    #[test]
    fn empty_body_becomes_null_description() {
        assert_eq!(description_of(""), None);
        assert_eq!(description_of("x"), Some("x".to_string()));
    }
}
