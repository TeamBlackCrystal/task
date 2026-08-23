use regex::Regex;
use sea_orm::entity::prelude::Json;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
    prelude::Uuid,
};
use std::sync::LazyLock;

use crate::error::AppError;
use entity::{
    labels, project_statuses, projects, task_activities, task_labels, tasks, tenants, users,
};

static MENTION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@([a-zA-Z0-9_-]+)").expect("mention regex"));

pub async fn record_activity<C: ConnectionTrait>(
    db: &C,
    task_id: Uuid,
    user_id: Option<Uuid>,
    event_type: &str,
    payload: Json,
) -> Result<(), AppError> {
    task_activities::ActiveModel {
        id: Set(Uuid::new_v4()),
        task_id: Set(task_id),
        user_id: Set(user_id),
        event_type: Set(event_type.to_string()),
        payload: Set(payload),
        created_at: Set(chrono::Utc::now().into()),
    }
    .insert(db)
    .await?;
    Ok(())
}

pub async fn status_name<C: ConnectionTrait>(db: &C, status_id: Uuid) -> Result<String, AppError> {
    project_statuses::Entity::find_by_id(status_id)
        .one(db)
        .await?
        .map(|s| s.name)
        .ok_or(AppError::NotFound)
}

/// タスクに現在付与されているラベルの (id, 名前) 一覧（名前・ID 順ソート済み）。
/// label_added / label_removed アクティビティの前後スナップショットに使う。
pub async fn task_label_entries<C: ConnectionTrait>(
    db: &C,
    task_id: Uuid,
) -> Result<Vec<(Uuid, String)>, AppError> {
    let label_ids: Vec<Uuid> = task_labels::Entity::find()
        .filter(task_labels::Column::TaskId.eq(task_id))
        .all(db)
        .await?
        .into_iter()
        .map(|tl| tl.label_id)
        .collect();
    let mut entries: Vec<(Uuid, String)> = labels::Entity::find()
        .filter(labels::Column::Id.is_in(label_ids))
        .all(db)
        .await?
        .into_iter()
        .map(|l| (l.id, l.name))
        .collect();
    entries.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    Ok(entries)
}

/// 前後スナップショットの差分から label_added / label_removed を 1 ラベルにつき
/// 1 件ずつ記録する（docs/features/tasks/4.collaboration.md の event_type 定義に対応）。
/// 集合に変化が無ければ何も記録しない。
pub async fn record_label_diff<C: ConnectionTrait>(
    db: &C,
    task_id: Uuid,
    user_id: Option<Uuid>,
    before: &[(Uuid, String)],
    after: &[(Uuid, String)],
) -> Result<(), AppError> {
    let before_ids: std::collections::HashSet<Uuid> = before.iter().map(|(id, _)| *id).collect();
    let after_ids: std::collections::HashSet<Uuid> = after.iter().map(|(id, _)| *id).collect();
    for (id, name) in after.iter().filter(|(id, _)| !before_ids.contains(id)) {
        record_activity(
            db,
            task_id,
            user_id,
            "label_added",
            serde_json::json!({ "label_id": id, "name": name }),
        )
        .await?;
    }
    for (id, name) in before.iter().filter(|(id, _)| !after_ids.contains(id)) {
        record_activity(
            db,
            task_id,
            user_id,
            "label_removed",
            serde_json::json!({ "label_id": id, "name": name }),
        )
        .await?;
    }
    Ok(())
}

pub fn priority_label(priority: tasks::TaskPriority) -> &'static str {
    match priority {
        tasks::TaskPriority::CriticalFire => "critical_fire",
        tasks::TaskPriority::Critical => "critical",
        tasks::TaskPriority::High => "high",
        tasks::TaskPriority::Medium => "medium",
        tasks::TaskPriority::Low => "low",
        tasks::TaskPriority::Trivial => "trivial",
    }
}

pub async fn extract_mentions<C: ConnectionTrait>(
    db: &C,
    body: &str,
    project_id: Uuid,
) -> Result<Vec<Uuid>, AppError> {
    // Collect unique usernames preserving first-occurrence order
    let mut seen = std::collections::HashSet::new();
    let usernames: Vec<&str> = MENTION_RE
        .captures_iter(body)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str()))
        .filter(|u| seen.insert(*u))
        .collect();

    if usernames.is_empty() {
        return Ok(vec![]);
    }

    // プロジェクト境界の外にいる人はメンションしても通知しない。
    // メンバー未指定のプロジェクトはテナントメンバー全員が対象になる（#568）
    let member_ids: std::collections::HashSet<Uuid> =
        crate::access::project_accessible_user_ids(db, project_id).await?;

    let tenant_owner_id: Option<Uuid> =
        if let Some(proj) = projects::Entity::find_by_id(project_id).one(db).await? {
            tenants::Entity::find_by_id(proj.tenant_id)
                .one(db)
                .await?
                .map(|t| t.owner_id)
        } else {
            None
        };

    // Single batch query for all mentioned users
    let matched = users::Entity::find()
        .filter(users::Column::Username.is_in(usernames))
        .all(db)
        .await?;

    let mut user_ids: Vec<Uuid> = Vec::new();
    let mut seen_ids: std::collections::HashSet<Uuid> = std::collections::HashSet::new();
    for u in matched {
        let is_allowed =
            member_ids.contains(&u.id) || tenant_owner_id.is_some_and(|oid| oid == u.id);
        if is_allowed && seen_ids.insert(u.id) {
            user_ids.push(u.id);
        }
    }
    Ok(user_ids)
}
