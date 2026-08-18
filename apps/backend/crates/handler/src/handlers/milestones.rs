use std::collections::HashSet;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use axum_valid::Valid;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, prelude::Uuid,
};

use crate::AppState;
use crate::error::AppError;
use crate::extractors::AuthUser;
use crate::openapi::CrudErrors;
use entity::{milestones, tasks};
use payload::milestones::*;
#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/",
    tag = "Milestones",
    summary = "マイルストーン一覧",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
    ),
    responses(
        (status = 200, description = "マイルストーン一覧", body = [MilestoneResponse]),
        CrudErrors,
    )
)]
pub async fn list_milestones(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<MilestoneResponse>>, AppError> {
    auth.require_scope(entity::scopes::Scope::ReadMilestone)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let list = milestones::Entity::find()
        .filter(milestones::Column::ProjectId.eq(project_id))
        .all(&state.db)
        .await?;
    Ok(Json(list.into_iter().map(Into::into).collect()))
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/",
    tag = "Milestones",
    summary = "マイルストーン作成",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
    ),
    request_body = CreateMilestoneRequest,
    responses(
        (status = 201, description = "作成されたマイルストーン", body = MilestoneResponse),
        CrudErrors,
    )
)]
pub async fn create_milestone(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id)): Path<(Uuid, Uuid)>,
    Valid(Json(payload)): Valid<Json<CreateMilestoneRequest>>,
) -> Result<(StatusCode, Json<MilestoneResponse>), AppError> {
    auth.require_scope(entity::scopes::Scope::WriteMilestone)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let model = milestones::ActiveModel {
        id: Set(Uuid::new_v4()),
        project_id: Set(project_id),
        name: Set(payload.name),
        description: Set(payload.description),
        due_date: Set(payload.due_date),
        created_by: Set(auth.user_id),
        created_at: Set(chrono::Utc::now().into()),
        updated_at: Set(chrono::Utc::now().into()),
    }
    .insert(&state.db)
    .await?;
    Ok((StatusCode::CREATED, Json(model.into())))
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "Milestones",
    summary = "マイルストーン取得（完了率含む）",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = Uuid, Path, description = "マイルストーンID"),
    ),
    responses(
        (status = 200, description = "マイルストーン詳細", body = MilestoneDetail),
        CrudErrors,
    )
)]
pub async fn get_milestone(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<MilestoneDetail>, AppError> {
    auth.require_scope(entity::scopes::Scope::ReadMilestone)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let milestone = milestones::Entity::find_by_id(id)
        .filter(milestones::Column::ProjectId.eq(project_id))
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    let all_tasks = tasks::Entity::find()
        .filter(tasks::Column::MilestoneId.eq(id))
        .filter(tasks::Column::DeletedAt.is_null())
        .all(&state.db)
        .await?;

    let total = all_tasks.len();
    let done = if total > 0 {
        // Fetch done statuses for this project
        let done_statuses: HashSet<Uuid> = entity::project_statuses::Entity::find()
            .filter(entity::project_statuses::Column::ProjectId.eq(project_id))
            .filter(entity::project_statuses::Column::IsDoneState.eq(true))
            .all(&state.db)
            .await?
            .into_iter()
            .map(|s| s.id)
            .collect();
        all_tasks
            .iter()
            .filter(|t| done_statuses.contains(&t.status_id))
            .count()
    } else {
        0
    };

    let progress_pct = (done * 100).checked_div(total).unwrap_or(0) as u32;

    Ok(Json(MilestoneDetail {
        milestone: milestone.into(),
        progress_pct,
        task_counts: TaskCounts { total, done },
    }))
}

#[axum::debug_handler]
#[utoipa::path(
    put,
    path = "/{id}",
    tag = "Milestones",
    summary = "マイルストーン更新",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = Uuid, Path, description = "マイルストーンID"),
    ),
    request_body = UpdateMilestoneRequest,
    responses(
        (status = 200, description = "更新後のマイルストーン", body = MilestoneResponse),
        CrudErrors,
    )
)]
pub async fn update_milestone(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id)): Path<(Uuid, Uuid, Uuid)>,
    Valid(Json(payload)): Valid<Json<UpdateMilestoneRequest>>,
) -> Result<Json<MilestoneResponse>, AppError> {
    auth.require_scope(entity::scopes::Scope::WriteMilestone)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let milestone = milestones::Entity::find_by_id(id)
        .filter(milestones::Column::ProjectId.eq(project_id))
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    let mut active: milestones::ActiveModel = milestone.into();
    if let Some(v) = payload.name {
        active.name = Set(v);
    }
    if payload.clear_description {
        active.description = Set(None);
    } else if let Some(v) = payload.description {
        active.description = Set(Some(v));
    }
    if let Some(v) = payload.due_date {
        active.due_date = Set(v);
    }
    active.updated_at = Set(chrono::Utc::now().into());
    Ok(Json(active.update(&state.db).await?.into()))
}

#[axum::debug_handler]
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "Milestones",
    summary = "マイルストーン削除",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = Uuid, Path, description = "マイルストーンID"),
    ),
    responses(
        (status = 204, description = "削除しました（タスクの milestone_id は NULL リセット）"),
        CrudErrors,
    )
)]
pub async fn delete_milestone(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    auth.require_scope(entity::scopes::Scope::WriteMilestone)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    // tasks.milestone_id cascades to NULL via FK ON DELETE SET NULL
    let result = milestones::Entity::delete_many()
        .filter(milestones::Column::Id.eq(id))
        .filter(milestones::Column::ProjectId.eq(project_id))
        .exec(&state.db)
        .await?;
    if result.rows_affected == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}
