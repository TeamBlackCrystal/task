use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use axum_valid::Valid;
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    TransactionTrait, prelude::Uuid,
};
use std::collections::HashMap;

use crate::AppState;
use crate::auth_helpers::is_tenant_owner;
use crate::error::AppError;
use crate::extractors::AuthUser;
use crate::handlers::tasks::resolve_task;
use crate::openapi::CrudErrors;
use entity::{task_activities, task_comments, users};
use payload::task_comments::*;
use service::notifications::{notify_comment_added, notify_mentioned};
use service::task_activities::{extract_mentions, record_activity};

fn comment_body(model: &task_comments::Model) -> Option<String> {
    if model.deleted_at.is_some() {
        None
    } else {
        Some(model.body.clone())
    }
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/{id}/comments",
    tag = "Tasks",
    summary = "コメント一覧（スレッド構造）",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = String, Path, description = "タスクID"),
    ),
    responses(
        (status = 200, description = "コメント一覧", body = CommentListResponse),
        CrudErrors,
    )
)]
pub async fn list_comments(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id)): Path<(Uuid, Uuid, String)>,
) -> Result<Json<CommentListResponse>, AppError> {
    auth.require_scope(entity::scopes::Scope::ReadTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let task = resolve_task(&state, tenant_id, project_id, &id).await?;

    let all = task_comments::Entity::find()
        .filter(task_comments::Column::TaskId.eq(task.id))
        .order_by_asc(task_comments::Column::CreatedAt)
        .all(&state.db)
        .await?;

    let user_ids: Vec<Uuid> = all.iter().map(|c| c.user_id).collect();
    let users_map: HashMap<Uuid, String> = if user_ids.is_empty() {
        HashMap::new()
    } else {
        users::Entity::find()
            .filter(users::Column::Id.is_in(user_ids))
            .all(&state.db)
            .await?
            .into_iter()
            .map(|u| (u.id, u.username))
            .collect()
    };

    let mut top_level: Vec<task_comments::Model> = Vec::new();
    let mut replies_by_parent: HashMap<Uuid, Vec<task_comments::Model>> = HashMap::new();

    for comment in all {
        if let Some(parent_id) = comment.parent_comment_id {
            replies_by_parent
                .entry(parent_id)
                .or_default()
                .push(comment);
        } else {
            top_level.push(comment);
        }
    }

    // トップレベル: 新しい順、リプライ: 古い順（スレッド内時系列）
    top_level.sort_by_key(|c| std::cmp::Reverse(c.created_at));

    let comments = top_level
        .into_iter()
        .map(|parent| {
            let user_name = users_map
                .get(&parent.user_id)
                .cloned()
                .unwrap_or_else(|| "unknown".into());
            let mut thread_replies = replies_by_parent.remove(&parent.id).unwrap_or_default();
            thread_replies.sort_by_key(|a| a.created_at);
            let replies = thread_replies
                .into_iter()
                .map(|reply| {
                    let reply_user = users_map
                        .get(&reply.user_id)
                        .cloned()
                        .unwrap_or_else(|| "unknown".into());
                    CommentReply {
                        id: reply.id,
                        user: CommentUser {
                            id: reply.user_id,
                            name: reply_user,
                        },
                        body: comment_body(&reply),
                        created_at: reply.created_at.with_timezone(&Utc),
                        updated_at: reply.updated_at.with_timezone(&Utc),
                        is_deleted: reply.deleted_at.is_some(),
                    }
                })
                .collect();
            CommentThread {
                id: parent.id,
                user: CommentUser {
                    id: parent.user_id,
                    name: user_name,
                },
                body: comment_body(&parent),
                replies,
                created_at: parent.created_at.with_timezone(&Utc),
                updated_at: parent.updated_at.with_timezone(&Utc),
                is_deleted: parent.deleted_at.is_some(),
            }
        })
        .collect();

    Ok(Json(CommentListResponse { comments }))
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/{id}/comments",
    tag = "Tasks",
    summary = "コメント投稿",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = String, Path, description = "タスクID"),
    ),
    request_body = CreateCommentRequest,
    responses(
        (status = 201, description = "作成されたコメント", body = TaskCommentResponse),
        CrudErrors,
    )
)]
pub async fn create_comment(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id)): Path<(Uuid, Uuid, String)>,
    Valid(Json(payload)): Valid<Json<CreateCommentRequest>>,
) -> Result<(StatusCode, Json<TaskCommentResponse>), AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let task = resolve_task(&state, tenant_id, project_id, &id).await?;

    if let Some(parent_id) = payload.parent_comment_id {
        let parent = task_comments::Entity::find_by_id(parent_id)
            .filter(task_comments::Column::TaskId.eq(task.id))
            .one(&state.db)
            .await?
            .ok_or(AppError::NotFound)?;
        if parent.parent_comment_id.is_some() {
            return Err(AppError::BadRequest);
        }
        if parent.deleted_at.is_some() {
            return Err(AppError::BadRequest);
        }
    }

    let mentions = extract_mentions(&state.db, &payload.body, project_id).await?;

    let txn = state.db.begin().await?;
    // created_at と updated_at は同一の now を入れる。frontend は
    // updated_at != created_at を「編集済み」と解釈するため、now() を
    // 2 回呼ぶとマイクロ秒差で未編集コメントに「(編集済み)」が付く
    let now: sea_orm::prelude::DateTimeWithTimeZone = chrono::Utc::now().into();
    let comment = task_comments::ActiveModel {
        id: Set(Uuid::new_v4()),
        task_id: Set(task.id),
        user_id: Set(auth.user_id),
        body: Set(payload.body),
        parent_comment_id: Set(payload.parent_comment_id),
        created_at: Set(now),
        updated_at: Set(now),
        deleted_at: Set(None),
    }
    .insert(&txn)
    .await?;

    record_activity(
        &txn,
        task.id,
        Some(auth.user_id),
        "comment_added",
        serde_json::json!({ "comment_id": comment.id }),
    )
    .await?;
    let actually_mentioned = notify_mentioned(
        &txn,
        project_id,
        task.id,
        &mentions,
        comment.id,
        auth.user_id,
    )
    .await?;
    notify_comment_added(
        &txn,
        project_id,
        task.id,
        comment.id,
        auth.user_id,
        &actually_mentioned,
    )
    .await?;
    txn.commit().await?;

    Ok((StatusCode::CREATED, Json(comment.into())))
}

#[axum::debug_handler]
#[utoipa::path(
    put,
    path = "/{id}/comments/{cid}",
    tag = "Tasks",
    summary = "コメント編集（投稿者本人のみ）",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = String, Path, description = "タスクID"),
        ("cid" = Uuid, Path, description = "コメントID"),
    ),
    request_body = UpdateCommentRequest,
    responses(
        (status = 200, description = "更新後のコメント", body = TaskCommentResponse),
        CrudErrors,
    )
)]
pub async fn update_comment(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id, cid)): Path<(Uuid, Uuid, String, Uuid)>,
    Valid(Json(payload)): Valid<Json<UpdateCommentRequest>>,
) -> Result<Json<TaskCommentResponse>, AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let task = resolve_task(&state, tenant_id, project_id, &id).await?;
    let comment = task_comments::Entity::find_by_id(cid)
        .filter(task_comments::Column::TaskId.eq(task.id))
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    if comment.deleted_at.is_some() {
        return Err(AppError::NotFound);
    }
    if comment.user_id != auth.user_id {
        return Err(AppError::Forbidden);
    }

    let old_body = comment.body.clone();
    let old_mentions = extract_mentions(&state.db, &old_body, project_id).await?;
    let new_mentions = extract_mentions(&state.db, &payload.body, project_id).await?;

    // 編集前から存在するメンションを除き、新規に追加されたメンションのみ通知
    let old_set: std::collections::HashSet<Uuid> = old_mentions.into_iter().collect();
    let added_mentions: Vec<Uuid> = new_mentions
        .into_iter()
        .filter(|id| !old_set.contains(id))
        .collect();

    let comment_id = comment.id;
    let txn = state.db.begin().await?;
    let mut active: task_comments::ActiveModel = comment.into();
    active.body = Set(payload.body);
    active.updated_at = Set(chrono::Utc::now().into());
    let updated = active.update(&txn).await?;
    record_activity(
        &txn,
        task.id,
        Some(auth.user_id),
        "comment_edited",
        serde_json::json!({ "comment_id": comment_id }),
    )
    .await?;
    // 編集時は新規メンションのみ通知する。notify_comment_added は呼ばない
    // （ウォッチャー全員への再通知は過剰なため、編集コメントは意図的に対象外）
    let _ = notify_mentioned(
        &txn,
        project_id,
        task.id,
        &added_mentions,
        comment_id,
        auth.user_id,
    )
    .await?;
    txn.commit().await?;
    Ok(Json(updated.into()))
}

#[axum::debug_handler]
#[utoipa::path(
    delete,
    path = "/{id}/comments/{cid}",
    tag = "Tasks",
    summary = "コメント削除（ソフト、投稿者 or テナントオーナー）",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = String, Path, description = "タスクID"),
        ("cid" = Uuid, Path, description = "コメントID"),
    ),
    responses(
        (status = 204, description = "削除しました"),
        CrudErrors,
    )
)]
pub async fn delete_comment(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id, cid)): Path<(Uuid, Uuid, String, Uuid)>,
) -> Result<StatusCode, AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let task = resolve_task(&state, tenant_id, project_id, &id).await?;
    let comment = task_comments::Entity::find_by_id(cid)
        .filter(task_comments::Column::TaskId.eq(task.id))
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    if comment.deleted_at.is_some() {
        return Err(AppError::NotFound);
    }
    let is_owner = is_tenant_owner(&state.db, tenant_id, auth.user_id).await?;
    if comment.user_id != auth.user_id && !is_owner {
        return Err(AppError::Forbidden);
    }
    let comment_id = comment.id;
    let task_id = task.id;
    let txn = state.db.begin().await?;
    let mut active: task_comments::ActiveModel = comment.into();
    active.deleted_at = Set(Some(chrono::Utc::now().into()));
    active.updated_at = Set(chrono::Utc::now().into());
    active.update(&txn).await?;
    record_activity(
        &txn,
        task_id,
        Some(auth.user_id),
        "comment_deleted",
        serde_json::json!({ "comment_id": comment_id }),
    )
    .await?;
    txn.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/{id}/activities",
    tag = "Tasks",
    summary = "アクティビティ一覧",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = String, Path, description = "タスクID"),
    ),
    responses(
        (status = 200, description = "アクティビティ一覧", body = ActivityListResponse),
        CrudErrors,
    )
)]
pub async fn list_activities(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id)): Path<(Uuid, Uuid, String)>,
) -> Result<Json<ActivityListResponse>, AppError> {
    auth.require_scope(entity::scopes::Scope::ReadTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let task = resolve_task(&state, tenant_id, project_id, &id).await?;

    let rows = task_activities::Entity::find()
        .filter(task_activities::Column::TaskId.eq(task.id))
        .order_by_desc(task_activities::Column::CreatedAt)
        .all(&state.db)
        .await?;

    let user_ids: Vec<Uuid> = rows.iter().filter_map(|a| a.user_id).collect();
    let users_map: HashMap<Uuid, String> = if user_ids.is_empty() {
        HashMap::new()
    } else {
        users::Entity::find()
            .filter(users::Column::Id.is_in(user_ids))
            .all(&state.db)
            .await?
            .into_iter()
            .map(|u| (u.id, u.username))
            .collect()
    };

    let activities = rows
        .into_iter()
        .map(|row| {
            let user = row.user_id.map(|uid| ActivityUser {
                id: uid,
                name: users_map
                    .get(&uid)
                    .cloned()
                    .unwrap_or_else(|| "unknown".into()),
            });
            ActivityItem {
                id: row.id,
                event_type: row.event_type,
                user,
                payload: row.payload.clone(),
                created_at: row.created_at.with_timezone(&Utc),
            }
        })
        .collect();

    Ok(Json(ActivityListResponse { activities }))
}
