//! 全文検索・バルク操作・保存済みビュー・ファイル添付。

use crate::AppState;
use crate::auth_helpers::{is_tenant_owner, require_project_access};
use crate::error::AppError;
use crate::extractors::AuthUser;
use crate::handlers::tasks::resolve_task;
use crate::openapi::CrudErrors;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use axum_valid::Valid;
use chrono::Utc;
use entity::{
    drive_files, labels, project_statuses, project_task_views, sprints, task_assignees,
    task_attachments, task_labels, tasks,
};
use payload::drive_files::content_url;
use payload::task_extensions::*;
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, Condition, ConnectionTrait, EntityTrait,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Statement, TransactionTrait,
    prelude::Uuid,
};
use service::db::is_postgres_unique_violation;
use service::task_activities::{
    record_activity, record_label_diff, status_name, task_label_entries,
};
use std::collections::HashSet;

const BULK_MAX_TASKS: usize = 100;

fn use_pg_bigm_search() -> bool {
    matches!(
        std::env::var("USE_PG_BIGM").as_deref(),
        Ok("1") | Ok("true") | Ok("True") | Ok("TRUE")
    )
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/search",
    tag = "Tasks",
    summary = "タスク全文検索",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        SearchTasksQuery,
    ),
    responses(
        (status = 200, description = "検索結果", body = SearchTasksResponse),
        CrudErrors,
    )
)]
pub async fn search_tasks(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id)): Path<(Uuid, Uuid)>,
    Query(q): Query<SearchTasksQuery>,
) -> Result<Json<SearchTasksResponse>, AppError> {
    auth.require_scope(entity::scopes::Scope::ReadTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;

    let query = q.q.trim();
    if query.is_empty() {
        return Err(AppError::BadRequest);
    }

    let limit = q.limit.min(100);
    let offset = q.offset;

    // pg_bigm は LIKE + gin_bigm_ops で使う拡張であり tsvector 設定名ではない。
    // USE_PG_BIGM=true の場合は ILIKE + gin_bigm_ops index 経路を使う。
    if use_pg_bigm_search() {
        search_tasks_ilike(&state, project_id, query, limit, offset).await
    } else {
        search_tasks_tsvector(&state, project_id, query, limit, offset).await
    }
}

fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

async fn search_tasks_ilike(
    state: &AppState,
    project_id: Uuid,
    query: &str,
    limit: u64,
    offset: u64,
) -> Result<Json<SearchTasksResponse>, AppError> {
    let pattern = format!("%{}%", escape_like(query));
    let base = tasks::Entity::find()
        .filter(tasks::Column::ProjectId.eq(project_id))
        .filter(tasks::Column::DeletedAt.is_null())
        .filter(Expr::cust_with_values(
            "(COALESCE(title, '') ILIKE $1 ESCAPE '\\' OR COALESCE(description, '') ILIKE $1 ESCAPE '\\')",
            [pattern.clone()],
        ));

    let total = base.clone().count(&state.db).await?;
    let rows = base
        .order_by_desc(tasks::Column::UpdatedAt)
        // 同じ更新時刻が並ぶと offset のページ境界で行が重複・欠落する
        .order_by_asc(tasks::Column::Id)
        .limit(limit)
        .offset(offset)
        .all(&state.db)
        .await?;

    let hits = rows
        .into_iter()
        .map(|t| SearchTaskHit {
            id: t.id,
            seq_id: t.seq_id,
            title: t.title.clone(),
            highlight: highlight_ilike(&t.title, t.description.as_deref(), query),
            score: 1.0,
        })
        .collect();

    Ok(Json(SearchTasksResponse { tasks: hits, total }))
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn chars_eq_ignore_case(a: char, b: char) -> bool {
    a.to_lowercase().eq(b.to_lowercase())
}

fn find_match_char_range(text: &str, query: &str) -> Option<(usize, usize)> {
    let query_chars: Vec<char> = query.chars().collect();
    if query_chars.is_empty() {
        return None;
    }
    let text_chars: Vec<char> = text.chars().collect();
    let q_len = query_chars.len();
    for start in 0..=text_chars.len().saturating_sub(q_len) {
        if text_chars[start..start + q_len]
            .iter()
            .zip(query_chars.iter())
            .all(|(a, b)| chars_eq_ignore_case(*a, *b))
        {
            return Some((start, start + q_len));
        }
    }
    None
}

fn chars_slice(s: &str, start: usize, end: usize) -> String {
    s.chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}

fn highlight_ilike(title: &str, description: Option<&str>, query: &str) -> String {
    let text = format!("{} {}", title, description.unwrap_or_default());
    if let Some((start_char, end_char)) = find_match_char_range(&text, query) {
        let total_chars = text.chars().count();
        let display_start = start_char.saturating_sub(60);
        let display_end = (end_char + 60).min(total_chars);
        let prefix_marker = if display_start > 0 { "…" } else { "" };
        let suffix_marker = if display_end < total_chars { "…" } else { "" };
        format!(
            "{prefix_marker}{}<em>{}</em>{}{suffix_marker}",
            html_escape(&chars_slice(&text, display_start, start_char)),
            html_escape(&chars_slice(&text, start_char, end_char)),
            html_escape(&chars_slice(&text, end_char, display_end)),
        )
    } else {
        html_escape(&text.chars().take(120).collect::<String>())
    }
}

async fn search_tasks_tsvector(
    state: &AppState,
    project_id: Uuid,
    query: &str,
    limit: u64,
    offset: u64,
) -> Result<Json<SearchTasksResponse>, AppError> {
    let count_sql = r#"
        SELECT COUNT(*)::bigint AS cnt
        FROM tasks
        WHERE project_id = $1
          AND deleted_at IS NULL
          AND search_vector @@ plainto_tsquery('pg_catalog.simple', $2)
    "#;
    let count_result = state
        .db
        .query_one_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            count_sql,
            [project_id.into(), query.into()],
        ))
        .await?;
    let total: i64 = count_result
        .and_then(|r| r.try_get_by_index(0).ok())
        .unwrap_or(0);

    let search_sql = r#"
        SELECT id, seq_id, title, description,
               ts_rank(search_vector, plainto_tsquery('pg_catalog.simple', $2))::real AS score
        FROM tasks
        WHERE project_id = $1
          AND deleted_at IS NULL
          AND search_vector @@ plainto_tsquery('pg_catalog.simple', $2)
        -- score は同値が頻発する。並びが一意でないと offset のページ境界で行が重複・欠落する
        ORDER BY score DESC, id ASC
        LIMIT $3 OFFSET $4
    "#;
    let rows = state
        .db
        .query_all_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            search_sql,
            [
                project_id.into(),
                query.into(),
                (limit as i64).into(),
                (offset as i64).into(),
            ],
        ))
        .await?;
    let hits = rows
        .into_iter()
        .filter_map(|row| {
            let title: String = row.try_get_by_index(2).ok()?;
            let description: Option<String> = row.try_get_by_index(3).ok().flatten();
            let highlight = highlight_ilike(&title, description.as_deref(), query);
            Some(SearchTaskHit {
                id: row.try_get_by_index(0).ok()?,
                seq_id: row.try_get_by_index(1).ok()?,
                title,
                score: row.try_get_by_index(4).ok()?,
                highlight,
            })
        })
        .collect();

    Ok(Json(SearchTasksResponse {
        tasks: hits,
        total: total as u64,
    }))
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/bulk",
    tag = "Tasks",
    summary = "タスク一括更新",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
    ),
    request_body = BulkUpdateRequest,
    responses(
        (status = 200, description = "一括更新結果", body = BulkUpdateResponse),
        CrudErrors,
    )
)]
pub async fn bulk_update_tasks(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id)): Path<(Uuid, Uuid)>,
    Valid(Json(payload)): Valid<Json<BulkUpdateRequest>>,
) -> Result<Json<BulkUpdateResponse>, AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;

    if payload.task_ids.len() > BULK_MAX_TASKS {
        return Err(AppError::BadRequest);
    }
    if let (Some(add), Some(remove)) = (
        &payload.update.add_label_ids,
        &payload.update.remove_label_ids,
    ) {
        let remove: HashSet<&Uuid> = remove.iter().collect();
        if add.iter().any(|id| remove.contains(id)) {
            return Err(AppError::BadRequest);
        }
    }

    let mut unique_ids = payload.task_ids.clone();
    unique_ids.sort();
    unique_ids.dedup();

    let mut updated = 0u32;
    let mut failed = Vec::new();

    for task_id in unique_ids {
        match apply_bulk_update(
            &state,
            tenant_id,
            project_id,
            task_id,
            auth.user_id,
            &payload.update,
        )
        .await
        {
            Ok(()) => updated += 1,
            Err(e) => failed.push(BulkFailure {
                task_id,
                reason: bulk_error_reason(&e),
            }),
        }
    }

    Ok(Json(BulkUpdateResponse { updated, failed }))
}

fn bulk_error_reason(err: &AppError) -> String {
    match err {
        AppError::NotFound => "not-found".into(),
        AppError::Forbidden => "forbidden".into(),
        AppError::Conflict => "conflict".into(),
        AppError::BadRequest => "bad-request".into(),
        AppError::BadRequestDetail(d) => d.clone(),
        _ => "error".into(),
    }
}

async fn apply_bulk_update(
    state: &AppState,
    tenant_id: Uuid,
    project_id: Uuid,
    task_id: Uuid,
    user_id: Uuid,
    update: &BulkUpdateFields,
) -> Result<(), AppError> {
    let task = tasks::Entity::find_by_id(task_id)
        .filter(tasks::Column::ProjectId.eq(project_id))
        .filter(tasks::Column::DeletedAt.is_null())
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    let txn = state.db.begin().await?;
    let snapshot = task.clone();
    let mut active: tasks::ActiveModel = task.into();

    if let Some(status_id) = update.status_id {
        // ステータスが変わらない場合は completed_at もアクティビティも更新しない
        if status_id != snapshot.status_id {
            let status = project_statuses::Entity::find_by_id(status_id)
                .filter(project_statuses::Column::ProjectId.eq(project_id))
                .one(&txn)
                .await?
                .ok_or(AppError::NotFound)?;
            active.status_id = Set(status_id);
            // 単体更新 API と同じルール: done 状態なら既存 completed_at を保持、なければ now()
            active.completed_at = if status.is_done_state {
                Set(Some(
                    snapshot
                        .completed_at
                        .unwrap_or_else(|| chrono::Utc::now().into()),
                ))
            } else {
                Set(None)
            };
            let from = status_name(&txn, snapshot.status_id).await?;
            let to = status_name(&txn, status_id).await?;
            record_activity(
                &txn,
                task_id,
                Some(user_id),
                "status_changed",
                serde_json::json!({ "from": from, "to": to }),
            )
            .await?;
        }
    }

    if update.clear_sprint_id {
        active.sprint_id = Set(None);
    } else if let Some(sprint_id) = update.sprint_id {
        sprints::Entity::find_by_id(sprint_id)
            .filter(sprints::Column::ProjectId.eq(project_id))
            .one(&txn)
            .await?
            .ok_or(AppError::NotFound)?;
        active.sprint_id = Set(Some(sprint_id));
    }

    active.updated_at = Set(chrono::Utc::now().into());
    active.update(&txn).await?;

    if let Some(assignee_id) = update.assignee_id {
        require_project_access(state, tenant_id, project_id, assignee_id).await?;
        let exists = task_assignees::Entity::find()
            .filter(task_assignees::Column::TaskId.eq(task_id))
            .filter(task_assignees::Column::UserId.eq(assignee_id))
            .one(&txn)
            .await?
            .is_some();
        if !exists {
            task_assignees::ActiveModel {
                id: Set(Uuid::new_v4()),
                task_id: Set(task_id),
                user_id: Set(assignee_id),
                role: Set("assignee".into()),
                assigned_at: Set(chrono::Utc::now().into()),
            }
            .insert(&txn)
            .await?;
            record_activity(
                &txn,
                task_id,
                Some(user_id),
                "assignee_added",
                serde_json::json!({ "user_id": assignee_id }),
            )
            .await?;
        }
    }

    // ラベル変更の前後スナップショット。実際に集合が変わったときだけ記録する。
    // ラベルを変える入力が増えたら、この判定式に足すこと。
    // ここを忘れると記録だけが静かに欠ける
    let labels_will_change = update.add_label_ids.is_some() || update.remove_label_ids.is_some();
    let before_labels = if labels_will_change {
        Some(task_label_entries(&txn, task_id).await?)
    } else {
        None
    };

    if let Some(ref label_ids) = update.add_label_ids {
        let mut unique = label_ids.clone();
        unique.sort();
        unique.dedup();
        if !unique.is_empty() {
            let in_project = labels::Entity::find()
                .filter(labels::Column::Id.is_in(unique.clone()))
                .filter(labels::Column::ProjectId.eq(project_id))
                .all(&txn)
                .await?;
            if in_project.len() != unique.len() {
                return Err(AppError::BadRequest);
            }
            for lid in unique {
                let exists = task_labels::Entity::find()
                    .filter(task_labels::Column::TaskId.eq(task_id))
                    .filter(task_labels::Column::LabelId.eq(lid))
                    .one(&txn)
                    .await?
                    .is_some();
                if !exists {
                    task_labels::ActiveModel {
                        task_id: Set(task_id),
                        label_id: Set(lid),
                    }
                    .insert(&txn)
                    .await?;
                }
            }
        }
    }

    if let Some(ref label_ids) = update.remove_label_ids {
        let mut unique = label_ids.clone();
        unique.sort();
        unique.dedup();
        if !unique.is_empty() {
            task_labels::Entity::delete_many()
                .filter(task_labels::Column::TaskId.eq(task_id))
                .filter(task_labels::Column::LabelId.is_in(unique))
                .exec(&txn)
                .await?;
        }
    }

    if let Some(before_labels) = before_labels {
        let after_labels = task_label_entries(&txn, task_id).await?;
        record_label_diff(&txn, task_id, Some(user_id), &before_labels, &after_labels).await?;
    }

    let linked = service::github::sync::mark_pending_push(&txn, task_id).await?;
    txn.commit().await?;
    if linked {
        crate::handlers::github::enqueue_issue_push(state, task_id).await;
    }
    Ok(())
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/",
    tag = "TaskViews",
    summary = "保存済みビュー一覧",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
    ),
    responses(
        (status = 200, description = "ビュー一覧", body = TaskViewListResponse),
        CrudErrors,
    )
)]
pub async fn list_task_views(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<TaskViewListResponse>, AppError> {
    auth.require_scope(entity::scopes::Scope::ReadTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;

    let views = project_task_views::Entity::find()
        .filter(project_task_views::Column::ProjectId.eq(project_id))
        .filter(
            Condition::any()
                .add(project_task_views::Column::CreatedBy.eq(auth.user_id))
                .add(project_task_views::Column::IsShared.eq(true)),
        )
        .order_by_asc(project_task_views::Column::Name)
        .all(&state.db)
        .await?;

    Ok(Json(TaskViewListResponse {
        views: views.into_iter().map(Into::into).collect(),
    }))
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/",
    tag = "TaskViews",
    summary = "保存済みビュー作成",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
    ),
    request_body = CreateTaskViewRequest,
    responses(
        (status = 201, description = "作成されたビュー", body = ProjectTaskViewResponse),
        CrudErrors,
    )
)]
pub async fn create_task_view(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id)): Path<(Uuid, Uuid)>,
    Valid(Json(payload)): Valid<Json<CreateTaskViewRequest>>,
) -> Result<(StatusCode, Json<ProjectTaskViewResponse>), AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;

    let model = project_task_views::ActiveModel {
        id: Set(Uuid::new_v4()),
        project_id: Set(project_id),
        created_by: Set(auth.user_id),
        name: Set(payload.name),
        is_shared: Set(payload.is_shared),
        filters: Set(payload.filters),
        sort: Set(payload.sort),
        view_type: Set(payload.view_type),
        created_at: Set(chrono::Utc::now().into()),
        updated_at: Set(chrono::Utc::now().into()),
    }
    .insert(&state.db)
    .await?;

    Ok((StatusCode::CREATED, Json(model.into())))
}

#[axum::debug_handler]
#[utoipa::path(
    patch,
    path = "/{view_id}",
    tag = "TaskViews",
    summary = "保存済みビュー更新",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("view_id" = Uuid, Path, description = "ビューID"),
    ),
    request_body = UpdateTaskViewRequest,
    responses(
        (status = 200, description = "更新されたビュー", body = ProjectTaskViewResponse),
        CrudErrors,
    )
)]
pub async fn update_task_view(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, view_id)): Path<(Uuid, Uuid, Uuid)>,
    Valid(Json(payload)): Valid<Json<UpdateTaskViewRequest>>,
) -> Result<Json<ProjectTaskViewResponse>, AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;

    let view = project_task_views::Entity::find_by_id(view_id)
        .filter(project_task_views::Column::ProjectId.eq(project_id))
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    let owner = is_tenant_owner(&state.db, tenant_id, auth.user_id).await?;
    if view.created_by != auth.user_id && !owner {
        return Err(AppError::Forbidden);
    }

    let mut active: project_task_views::ActiveModel = view.into();
    if let Some(name) = payload.name {
        active.name = Set(name);
    }
    if let Some(is_shared) = payload.is_shared {
        active.is_shared = Set(is_shared);
    }
    if let Some(filters) = payload.filters {
        active.filters = Set(filters);
    }
    if let Some(sort) = payload.sort {
        active.sort = Set(sort);
    }
    if let Some(view_type) = payload.view_type {
        active.view_type = Set(view_type);
    }
    active.updated_at = Set(chrono::Utc::now().into());

    let updated = active.update(&state.db).await?;
    Ok(Json(updated.into()))
}

#[axum::debug_handler]
#[utoipa::path(
    delete,
    path = "/{view_id}",
    tag = "TaskViews",
    summary = "保存済みビュー削除",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("view_id" = Uuid, Path, description = "ビューID"),
    ),
    responses(
        (status = 204, description = "削除成功"),
        CrudErrors,
    )
)]
pub async fn delete_task_view(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, view_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;

    let view = project_task_views::Entity::find_by_id(view_id)
        .filter(project_task_views::Column::ProjectId.eq(project_id))
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    let owner = is_tenant_owner(&state.db, tenant_id, auth.user_id).await?;
    if view.created_by != auth.user_id && !owner {
        return Err(AppError::Forbidden);
    }

    project_task_views::Entity::delete_by_id(view_id)
        .exec(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/{id}/attachments",
    tag = "Tasks",
    summary = "タスク添付ファイル一覧",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = String, Path, description = "タスクID（UUID または連番ID）"),
    ),
    responses(
        (status = 200, description = "添付ファイル一覧", body = TaskAttachmentListResponse),
        CrudErrors,
    )
)]
pub async fn list_task_attachments(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id)): Path<(Uuid, Uuid, String)>,
) -> Result<Json<TaskAttachmentListResponse>, AppError> {
    auth.require_scope(entity::scopes::Scope::ReadTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let task = resolve_task(&state, tenant_id, project_id, &id).await?;

    let rows = task_attachments::Entity::find()
        .filter(task_attachments::Column::TaskId.eq(task.id))
        .order_by_desc(task_attachments::Column::CreatedAt)
        .find_also_related(drive_files::Entity)
        .all(&state.db)
        .await?;

    let attachments = rows
        .into_iter()
        .map(|(row, file)| {
            let file = file.ok_or(AppError::NotFound)?;
            Ok(TaskAttachmentResponse {
                id: row.id,
                drive_file_id: row.drive_file_id,
                name: file.name,
                mime_type: file.mime_type,
                size: file.size,
                url: content_url(file.id),
                created_at: row.created_at.with_timezone(&Utc),
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;

    Ok(Json(TaskAttachmentListResponse { attachments }))
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/{id}/attachments",
    tag = "Tasks",
    summary = "タスクにファイルを添付",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = String, Path, description = "タスクID（UUID または連番ID）"),
    ),
    request_body = AttachFileRequest,
    responses(
        (status = 201, description = "添付完了", body = TaskAttachmentResponse),
        CrudErrors,
    )
)]
pub async fn attach_task_file(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id)): Path<(Uuid, Uuid, String)>,
    Valid(Json(payload)): Valid<Json<AttachFileRequest>>,
) -> Result<(StatusCode, Json<TaskAttachmentResponse>), AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let task = resolve_task(&state, tenant_id, project_id, &id).await?;

    let file = drive_files::Entity::find_by_id(payload.drive_file_id)
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    if file.tenant_id != tenant_id {
        return Err(AppError::Forbidden);
    }
    if let Some(pid) = file.project_id
        && pid != project_id
    {
        return Err(AppError::Forbidden);
    }

    let already_attached = task_attachments::Entity::find()
        .filter(task_attachments::Column::TaskId.eq(task.id))
        .filter(task_attachments::Column::DriveFileId.eq(payload.drive_file_id))
        .one(&state.db)
        .await?
        .is_some();
    if already_attached {
        return Err(AppError::Conflict);
    }

    let model = match (task_attachments::ActiveModel {
        id: Set(Uuid::new_v4()),
        task_id: Set(task.id),
        drive_file_id: Set(payload.drive_file_id),
        created_by: Set(auth.user_id),
        created_at: Set(chrono::Utc::now().into()),
    })
    .insert(&state.db)
    .await
    {
        Ok(model) => model,
        Err(e) if is_postgres_unique_violation(&e) => return Err(AppError::Conflict),
        Err(e) => return Err(e.into()),
    };

    Ok((
        StatusCode::CREATED,
        Json(TaskAttachmentResponse {
            id: model.id,
            drive_file_id: file.id,
            name: file.name,
            mime_type: file.mime_type,
            size: file.size,
            url: content_url(file.id),
            created_at: model.created_at.with_timezone(&Utc),
        }),
    ))
}

#[axum::debug_handler]
#[utoipa::path(
    delete,
    path = "/{id}/attachments/{attachment_id}",
    tag = "Tasks",
    summary = "タスク添付を解除",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = String, Path, description = "タスクID（UUID または連番ID）"),
        ("attachment_id" = Uuid, Path, description = "添付ID"),
    ),
    responses(
        (status = 204, description = "解除成功"),
        CrudErrors,
    )
)]
pub async fn detach_task_file(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id, attachment_id)): Path<(Uuid, Uuid, String, Uuid)>,
) -> Result<StatusCode, AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let task = resolve_task(&state, tenant_id, project_id, &id).await?;

    let attachment = task_attachments::Entity::find()
        .filter(task_attachments::Column::Id.eq(attachment_id))
        .filter(task_attachments::Column::TaskId.eq(task.id))
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    let owner = is_tenant_owner(&state.db, tenant_id, auth.user_id).await?;
    if attachment.created_by != auth.user_id && !owner {
        return Err(AppError::Forbidden);
    }

    task_attachments::Entity::delete_by_id(attachment.id)
        .exec(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
