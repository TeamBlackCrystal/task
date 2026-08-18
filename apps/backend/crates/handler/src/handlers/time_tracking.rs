use crate::AppState;
use crate::auth_helpers::is_tenant_owner;
use crate::error::AppError;
use crate::extractors::AuthUser;
use crate::handlers::tasks::resolve_task;
use crate::openapi::CrudErrors;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use axum_valid::Valid;
use chrono::{Duration, Utc};
use entity::{task_timers, time_logs, users};
use payload::time_tracking::*;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DbErr, EntityTrait,
    FromQueryResult, QueryFilter, QueryOrder, Statement, prelude::Uuid,
};
use service::db::{is_postgres_unique_violation, with_transaction};

#[derive(Debug, FromQueryResult)]
struct DeletedTimerRow {
    started_at: chrono::DateTime<Utc>,
}

fn elapsed_minutes_from_start(started_at: chrono::DateTime<Utc>) -> i32 {
    let secs = (Utc::now() - started_at).num_seconds();
    ((secs + 59) / 60).max(1) as i32
}

async fn require_log_owner_or_tenant_owner(
    state: &AppState,
    tenant_id: Uuid,
    log: &time_logs::Model,
    auth_user_id: Uuid,
    allow_owner_delete: bool,
) -> Result<(), AppError> {
    if log.user_id == auth_user_id {
        return Ok(());
    }
    if allow_owner_delete && is_tenant_owner(&state.db, tenant_id, auth_user_id).await? {
        return Ok(());
    }
    Err(AppError::Forbidden)
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/{id}/time-logs",
    tag = "Tasks",
    summary = "作業時間ログ一覧",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = String, Path, description = "タスクID"),
    ),
    responses(
        (status = 200, description = "ログ一覧", body = [TimeLogResponse]),
        CrudErrors,
    )
)]
pub async fn list_time_logs(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id)): Path<(Uuid, Uuid, String)>,
) -> Result<Json<Vec<TimeLogResponse>>, AppError> {
    auth.require_scope(entity::scopes::Scope::ReadTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let task = resolve_task(&state, tenant_id, project_id, &id).await?;
    let logs = time_logs::Entity::find()
        .filter(time_logs::Column::TaskId.eq(task.id))
        .order_by_desc(time_logs::Column::LoggedAt)
        .order_by_desc(time_logs::Column::CreatedAt)
        .all(&state.db)
        .await?;
    Ok(Json(logs.into_iter().map(Into::into).collect()))
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/{id}/time-logs",
    tag = "Tasks",
    summary = "手動作業時間ログ追加",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = String, Path, description = "タスクID"),
    ),
    request_body = CreateTimeLogRequest,
    responses(
        (status = 201, description = "作成されたログ", body = TimeLogResponse),
        CrudErrors,
    )
)]
pub async fn create_time_log(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id)): Path<(Uuid, Uuid, String)>,
    Valid(Json(payload)): Valid<Json<CreateTimeLogRequest>>,
) -> Result<(StatusCode, Json<TimeLogResponse>), AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let task = resolve_task(&state, tenant_id, project_id, &id).await?;
    let log = time_logs::ActiveModel {
        id: Set(Uuid::new_v4()),
        task_id: Set(task.id),
        user_id: Set(auth.user_id),
        logged_minutes: Set(payload.logged_minutes),
        logged_at: Set(payload.logged_at),
        note: Set(payload.note),
        created_at: Set(Utc::now().into()),
    }
    .insert(&state.db)
    .await?;
    Ok((StatusCode::CREATED, Json(log.into())))
}

#[axum::debug_handler]
#[utoipa::path(
    put,
    path = "/{id}/time-logs/{log_id}",
    tag = "Tasks",
    summary = "作業時間ログ編集（自分のログのみ）",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = String, Path, description = "タスクID"),
        ("log_id" = Uuid, Path, description = "ログID"),
    ),
    request_body = UpdateTimeLogRequest,
    responses(
        (status = 200, description = "更新後のログ", body = TimeLogResponse),
        CrudErrors,
    )
)]
pub async fn update_time_log(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id, log_id)): Path<(Uuid, Uuid, String, Uuid)>,
    Valid(Json(payload)): Valid<Json<UpdateTimeLogRequest>>,
) -> Result<Json<TimeLogResponse>, AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let task = resolve_task(&state, tenant_id, project_id, &id).await?;
    let log = time_logs::Entity::find_by_id(log_id)
        .filter(time_logs::Column::TaskId.eq(task.id))
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    require_log_owner_or_tenant_owner(&state, tenant_id, &log, auth.user_id, false).await?;
    let mut active: time_logs::ActiveModel = log.into();
    if let Some(minutes) = payload.logged_minutes {
        active.logged_minutes = Set(minutes);
    }
    if let Some(date) = payload.logged_at {
        active.logged_at = Set(date);
    }
    if payload.clear_note {
        active.note = Set(None);
    } else if let Some(note) = payload.note {
        active.note = Set(Some(note));
    }
    Ok(Json(active.update(&state.db).await?.into()))
}

#[axum::debug_handler]
#[utoipa::path(
    delete,
    path = "/{id}/time-logs/{log_id}",
    tag = "Tasks",
    summary = "作業時間ログ削除（自分 or テナントオーナー）",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = String, Path, description = "タスクID"),
        ("log_id" = Uuid, Path, description = "ログID"),
    ),
    responses(
        (status = 204, description = "削除しました"),
        CrudErrors,
    )
)]
pub async fn delete_time_log(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id, log_id)): Path<(Uuid, Uuid, String, Uuid)>,
) -> Result<StatusCode, AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let task = resolve_task(&state, tenant_id, project_id, &id).await?;
    let log = time_logs::Entity::find_by_id(log_id)
        .filter(time_logs::Column::TaskId.eq(task.id))
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    require_log_owner_or_tenant_owner(&state, tenant_id, &log, auth.user_id, true).await?;
    time_logs::Entity::delete_by_id(log_id)
        .exec(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/{id}/time-logs/summary",
    tag = "Tasks",
    summary = "作業時間サマリー",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = String, Path, description = "タスクID"),
    ),
    responses(
        (status = 200, description = "工数サマリー", body = TimeLogSummaryResponse),
        CrudErrors,
    )
)]
pub async fn get_time_summary(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id)): Path<(Uuid, Uuid, String)>,
) -> Result<Json<TimeLogSummaryResponse>, AppError> {
    auth.require_scope(entity::scopes::Scope::ReadTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let task = resolve_task(&state, tenant_id, project_id, &id).await?;
    let logs = time_logs::Entity::find()
        .filter(time_logs::Column::TaskId.eq(task.id))
        .all(&state.db)
        .await?;

    let mut by_user_map: std::collections::HashMap<Uuid, i32> = std::collections::HashMap::new();
    let mut actual_minutes = 0i32;
    for log in &logs {
        actual_minutes += log.logged_minutes;
        *by_user_map.entry(log.user_id).or_insert(0) += log.logged_minutes;
    }

    let user_ids: Vec<Uuid> = by_user_map.keys().copied().collect();
    let user_rows = if user_ids.is_empty() {
        vec![]
    } else {
        users::Entity::find()
            .filter(users::Column::Id.is_in(user_ids))
            .all(&state.db)
            .await?
    };
    let user_names: std::collections::HashMap<Uuid, String> =
        user_rows.into_iter().map(|u| (u.id, u.username)).collect();

    let by_user = by_user_map
        .into_iter()
        .map(|(user_id, minutes)| UserTimeSummary {
            name: user_names
                .get(&user_id)
                .cloned()
                .unwrap_or_else(|| "unknown".into()),
            user_id,
            minutes,
        })
        .collect();

    let estimated = task.estimated_minutes;
    let remaining = estimated.map(|e| e - actual_minutes);
    let is_over = estimated.is_some_and(|e| actual_minutes > e);

    Ok(Json(TimeLogSummaryResponse {
        estimated_minutes: estimated,
        actual_minutes,
        remaining_minutes: remaining,
        is_over,
        by_user,
    }))
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/{id}/timer/start",
    tag = "Tasks",
    summary = "タイマー開始",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = String, Path, description = "タスクID"),
    ),
    responses(
        (status = 201, description = "開始されたタイマー", body = TaskTimerResponse),
        CrudErrors,
    )
)]
pub async fn start_timer(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id)): Path<(Uuid, Uuid, String)>,
) -> Result<(StatusCode, Json<TaskTimerResponse>), AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let task = resolve_task(&state, tenant_id, project_id, &id).await?;
    let existing = task_timers::Entity::find()
        .filter(task_timers::Column::TaskId.eq(task.id))
        .filter(task_timers::Column::UserId.eq(auth.user_id))
        .one(&state.db)
        .await?;
    if existing.is_some() {
        return Err(AppError::Conflict);
    }
    let timer = match (task_timers::ActiveModel {
        task_id: Set(task.id),
        user_id: Set(auth.user_id),
        started_at: Set(Utc::now().into()),
    })
    .insert(&state.db)
    .await
    {
        Ok(timer) => timer,
        Err(DbErr::RecordNotInserted) => return Err(AppError::Conflict),
        Err(e) if is_postgres_unique_violation(&e) => return Err(AppError::Conflict),
        Err(e) => return Err(e.into()),
    };
    Ok((StatusCode::CREATED, Json(timer.into())))
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/{id}/timer/stop",
    tag = "Tasks",
    summary = "タイマー停止 → ログ生成",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = String, Path, description = "タスクID"),
    ),
    responses(
        (status = 200, description = "生成されたログ", body = TimeLogResponse),
        CrudErrors,
    )
)]
pub async fn stop_timer(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id)): Path<(Uuid, Uuid, String)>,
) -> Result<Json<TimeLogResponse>, AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let task = resolve_task(&state, tenant_id, project_id, &id).await?;
    let task_id = task.id;
    let user_id = auth.user_id;

    let log = with_transaction::<time_logs::Model, AppError, _>(&state.db, |txn| {
        Box::pin(async move {
            let deleted = DeletedTimerRow::find_by_statement(Statement::from_sql_and_values(
                txn.get_database_backend(),
                "DELETE FROM task_timers \
                 WHERE task_id = $1 AND user_id = $2 \
                 RETURNING started_at",
                vec![task_id.into(), user_id.into()],
            ))
            .one(txn)
            .await?;

            let timer = match deleted {
                Some(row) => row,
                None => return Err(AppError::Conflict),
            };
            let logged_minutes = elapsed_minutes_from_start(timer.started_at);
            let logged_at = (Utc::now() + Duration::hours(9)).date_naive();
            let log = time_logs::ActiveModel {
                id: Set(Uuid::new_v4()),
                task_id: Set(task_id),
                user_id: Set(user_id),
                logged_minutes: Set(logged_minutes),
                logged_at: Set(logged_at),
                note: Set(None),
                created_at: Set(Utc::now().into()),
            }
            .insert(txn)
            .await?;
            Ok(log)
        })
    })
    .await?;

    Ok(Json(log.into()))
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/{id}/timer/status",
    tag = "Tasks",
    summary = "タイマー状態取得",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = String, Path, description = "タスクID"),
    ),
    responses(
        (status = 200, description = "タイマー状態", body = TimerStatusResponse),
        CrudErrors,
    )
)]
pub async fn get_timer_status(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id)): Path<(Uuid, Uuid, String)>,
) -> Result<Json<TimerStatusResponse>, AppError> {
    auth.require_scope(entity::scopes::Scope::ReadTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let task = resolve_task(&state, tenant_id, project_id, &id).await?;
    let timer = task_timers::Entity::find()
        .filter(task_timers::Column::TaskId.eq(task.id))
        .filter(task_timers::Column::UserId.eq(auth.user_id))
        .one(&state.db)
        .await?;
    Ok(Json(match timer {
        Some(t) => TimerStatusResponse {
            is_running: true,
            started_at: Some(t.started_at.with_timezone(&Utc)),
            elapsed_minutes: Some(elapsed_minutes_from_start(t.started_at.with_timezone(&Utc))),
        },
        None => TimerStatusResponse {
            is_running: false,
            started_at: None,
            elapsed_minutes: None,
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn elapsed_minutes_from_start_zero_seconds() {
        assert_eq!(elapsed_minutes_from_start(Utc::now()), 1);
    }

    #[test]
    fn elapsed_minutes_from_start_fifty_nine_seconds() {
        assert_eq!(
            elapsed_minutes_from_start(Utc::now() - Duration::seconds(59)),
            1
        );
    }

    #[test]
    fn elapsed_minutes_from_start_sixty_seconds() {
        assert_eq!(
            elapsed_minutes_from_start(Utc::now() - Duration::seconds(60)),
            1
        );
    }

    #[test]
    fn elapsed_minutes_from_start_sixty_one_seconds() {
        assert_eq!(
            elapsed_minutes_from_start(Utc::now() - Duration::seconds(61)),
            2
        );
    }

    #[test]
    fn elapsed_minutes_from_start_future_timestamp() {
        assert_eq!(
            elapsed_minutes_from_start(Utc::now() + Duration::seconds(30)),
            1
        );
    }
}
