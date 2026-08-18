use sea_orm::entity::prelude::Json;
use sea_orm::sea_query::OnConflict;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
    prelude::Uuid,
};
use std::collections::HashSet;

use crate::error::AppError;
use entity::{notification_settings, notifications, projects, task_watchers, tenants, users};

// 定数本体は common へ移動（DTO からも参照するため）。既存の参照パス互換用に再公開。
pub use common::notifications::{
    DEFAULT_IN_APP_EVENTS, KNOWN_EVENT_TYPES, TYPE_ASSIGNED, TYPE_COMMENT_ADDED, TYPE_MENTIONED,
    TYPE_STATUS_CHANGED,
};

pub async fn ensure_watcher<C: ConnectionTrait>(
    db: &C,
    task_id: Uuid,
    user_id: Uuid,
) -> Result<(), AppError> {
    task_watchers::Entity::insert(task_watchers::ActiveModel {
        task_id: Set(task_id),
        user_id: Set(user_id),
        created_at: Set(chrono::Utc::now().into()),
    })
    .on_conflict(
        OnConflict::columns([task_watchers::Column::TaskId, task_watchers::Column::UserId])
            .do_nothing()
            .to_owned(),
    )
    .exec_without_returning(db)
    .await?;
    Ok(())
}

async fn in_app_enabled<C: ConnectionTrait>(
    db: &C,
    user_id: Uuid,
    project_id: Uuid,
    event_type: &str,
) -> Result<bool, AppError> {
    let settings = notification_settings::Entity::find()
        .filter(notification_settings::Column::UserId.eq(user_id))
        .filter(notification_settings::Column::ProjectId.eq(project_id))
        .one(db)
        .await?;

    let events: Vec<String> = match settings {
        Some(s) => s.in_app_events,
        None => DEFAULT_IN_APP_EVENTS
            .iter()
            .map(|e| (*e).to_string())
            .collect(),
    };
    Ok(events.iter().any(|e| e == event_type))
}

pub async fn create_notification<C: ConnectionTrait>(
    db: &C,
    user_id: Uuid,
    task_id: Option<Uuid>,
    notification_type: &str,
    payload: Json,
) -> Result<(), AppError> {
    notifications::ActiveModel {
        id: Set(Uuid::new_v4()),
        user_id: Set(user_id),
        task_id: Set(task_id),
        notification_type: Set(notification_type.to_string()),
        payload: Set(payload),
        read_at: Set(None),
        created_at: Set(chrono::Utc::now().into()),
    }
    .insert(db)
    .await?;
    Ok(())
}

async fn notify_user_if_enabled<C: ConnectionTrait>(
    db: &C,
    user_id: Uuid,
    project_id: Uuid,
    task_id: Uuid,
    notification_type: &str,
    payload: Json,
) -> Result<(), AppError> {
    if !in_app_enabled(db, user_id, project_id, notification_type).await? {
        return Ok(());
    }
    create_notification(db, user_id, Some(task_id), notification_type, payload).await
}

pub async fn notify_assigned<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    task_id: Uuid,
    assignee_id: Uuid,
    assigned_by: Uuid,
    role: &str,
) -> Result<(), AppError> {
    ensure_watcher(db, task_id, assignee_id).await?;
    let assigner_name = users::Entity::find_by_id(assigned_by)
        .one(db)
        .await?
        .map(|u| u.username)
        .unwrap_or_else(|| "unknown".into());
    notify_user_if_enabled(
        db,
        assignee_id,
        project_id,
        task_id,
        TYPE_ASSIGNED,
        serde_json::json!({
            "assigned_by": assigner_name,
            "role": role,
        }),
    )
    .await
}

pub async fn notify_watchers<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    task_id: Uuid,
    notification_type: &str,
    payload: Json,
    exclude: &[Uuid],
) -> Result<(), AppError> {
    let exclude_set: HashSet<Uuid> = exclude.iter().copied().collect();

    // プロジェクトに入れないユーザーには通知しない（テナントオーナーは除外しない）。
    // メンバー未指定のプロジェクトはテナントメンバー全員が宛先になる（#568）
    let member_ids: HashSet<Uuid> =
        crate::access::project_accessible_user_ids(db, project_id).await?;

    // テナントオーナーは project_members に入っていなくてもウォッチャー通知を受け取る
    let tenant_id = projects::Entity::find_by_id(project_id)
        .one(db)
        .await?
        .map(|p| p.tenant_id);
    let tenant_owner_id: Option<Uuid> = if let Some(tid) = tenant_id {
        tenants::Entity::find_by_id(tid)
            .one(db)
            .await?
            .map(|t| t.owner_id)
    } else {
        None
    };

    let watchers = task_watchers::Entity::find()
        .filter(task_watchers::Column::TaskId.eq(task_id))
        .all(db)
        .await?;
    for watcher in watchers {
        let is_accessible =
            member_ids.contains(&watcher.user_id) || tenant_owner_id == Some(watcher.user_id);
        if exclude_set.contains(&watcher.user_id) || !is_accessible {
            continue;
        }
        notify_user_if_enabled(
            db,
            watcher.user_id,
            project_id,
            task_id,
            notification_type,
            payload.clone(),
        )
        .await?;
    }
    Ok(())
}

/// メンション通知を送信し、実際に通知が届いたユーザーのIDリストを返す。
/// 返値は notify_comment_added の除外リストに使うこと。
pub async fn notify_mentioned<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    task_id: Uuid,
    mentioned_user_ids: &[Uuid],
    comment_id: Uuid,
    author_id: Uuid,
) -> Result<Vec<Uuid>, AppError> {
    let author_name = users::Entity::find_by_id(author_id)
        .one(db)
        .await?
        .map(|u| u.username)
        .unwrap_or_else(|| "unknown".into());
    let payload: Json = serde_json::json!({
        "comment_id": comment_id,
        "author": author_name,
    });
    let mut notified = Vec::new();
    for user_id in mentioned_user_ids {
        if *user_id == author_id {
            continue;
        }
        // メンション = 関心あり → notify_assigned と同様にウォッチャーへ自動追加
        ensure_watcher(db, task_id, *user_id).await?;
        if in_app_enabled(db, *user_id, project_id, TYPE_MENTIONED).await? {
            create_notification(db, *user_id, Some(task_id), TYPE_MENTIONED, payload.clone())
                .await?;
            notified.push(*user_id);
        }
    }
    Ok(notified)
}

pub async fn notify_comment_added<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    task_id: Uuid,
    comment_id: Uuid,
    author_id: Uuid,
    mentioned_user_ids: &[Uuid],
) -> Result<(), AppError> {
    let author_name = users::Entity::find_by_id(author_id)
        .one(db)
        .await?
        .map(|u| u.username)
        .unwrap_or_else(|| "unknown".into());
    let payload: Json = serde_json::json!({
        "comment_id": comment_id,
        "author": author_name,
    });
    let mut exclude = vec![author_id];
    exclude.extend_from_slice(mentioned_user_ids);
    notify_watchers(
        db,
        project_id,
        task_id,
        TYPE_COMMENT_ADDED,
        payload,
        &exclude,
    )
    .await
}

pub async fn notify_status_changed<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    task_id: Uuid,
    actor_id: Uuid,
    from: &str,
    to: &str,
) -> Result<(), AppError> {
    let actor_name = users::Entity::find_by_id(actor_id)
        .one(db)
        .await?
        .map(|u| u.username)
        .unwrap_or_else(|| "unknown".into());
    notify_watchers(
        db,
        project_id,
        task_id,
        TYPE_STATUS_CHANGED,
        serde_json::json!({
            "from": from,
            "to": to,
            "changed_by": actor_name,
        }),
        &[actor_id],
    )
    .await
}
