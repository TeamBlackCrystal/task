use crate::AppState;
use crate::error::AppError;
use crate::extractors::AuthUser;
use crate::openapi::CrudErrors;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use axum_valid::Valid;
use entity::labels;
use payload::labels::*;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, TransactionTrait,
    prelude::Uuid,
};
use validator::Validate;

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/",
    tag = "Labels",
    summary = "プロジェクトのラベル一覧",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
    ),
    responses(
        (status = 200, description = "ラベル一覧", body = [LabelResponse]),
        CrudErrors,
    )
)]
pub async fn list_labels(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<LabelResponse>>, AppError> {
    auth.require_scope(entity::scopes::Scope::ReadTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let list = labels::Entity::find()
        .filter(labels::Column::ProjectId.eq(project_id))
        .all(&state.db)
        .await?;
    Ok(Json(list.into_iter().map(Into::into).collect()))
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/",
    tag = "Labels",
    summary = "ラベル作成",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
    ),
    request_body = CreateLabelRequest,
    responses(
        (status = 201, description = "作成されたラベル", body = LabelResponse),
        CrudErrors,
    )
)]
pub async fn create_label(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id)): Path<(Uuid, Uuid)>,
    Valid(Json(payload)): Valid<Json<CreateLabelRequest>>,
) -> Result<(StatusCode, Json<LabelResponse>), AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let label = labels::ActiveModel {
        id: Set(Uuid::new_v4()),
        name: Set(payload.name),
        description: Set(payload.description),
        color: Set(payload.color),
        icon_url: Set(payload.icon_url),
        project_id: Set(Some(project_id)),
    }
    .insert(&state.db)
    .await?;
    Ok((StatusCode::CREATED, Json(label.into())))
}

#[axum::debug_handler]
#[utoipa::path(
    put,
    path = "/{id}",
    tag = "Labels",
    summary = "ラベル更新",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = Uuid, Path, description = "ラベルID"),
    ),
    request_body = UpdateLabelRequest,
    responses(
        (status = 200, description = "更新後のラベル", body = LabelResponse),
        CrudErrors,
    )
)]
pub async fn update_label(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id)): Path<(Uuid, Uuid, Uuid)>,
    Valid(Json(payload)): Valid<Json<UpdateLabelRequest>>,
) -> Result<Json<LabelResponse>, AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let label = labels::Entity::find_by_id(id)
        .filter(labels::Column::ProjectId.eq(project_id))
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    let mut active: labels::ActiveModel = label.into();
    if let Some(v) = payload.name {
        active.name = Set(v);
    }
    if let Some(v) = payload.description {
        active.description = Set(v);
    }
    if let Some(v) = payload.color {
        active.color = Set(v);
    }
    if payload.clear_icon_url {
        active.icon_url = Set(None);
    } else if let Some(v) = payload.icon_url {
        active.icon_url = Set(Some(v));
    }
    Ok(Json(active.update(&state.db).await?.into()))
}

#[axum::debug_handler]
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "Labels",
    summary = "ラベル削除",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = Uuid, Path, description = "ラベルID"),
    ),
    responses(
        (status = 204, description = "削除しました"),
        CrudErrors,
    )
)]
pub async fn delete_label(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let result = labels::Entity::delete_many()
        .filter(labels::Column::Id.eq(id))
        .filter(labels::Column::ProjectId.eq(project_id))
        .exec(&state.db)
        .await?;
    if result.rows_affected == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/export",
    tag = "Labels",
    summary = "ラベルを JSON エクスポート",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
    ),
    responses(
        (status = 200, description = "エクスポートデータ", body = LabelExport),
        CrudErrors,
    )
)]
pub async fn export_labels(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<LabelExport>, AppError> {
    auth.require_scope(entity::scopes::Scope::ReadTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let list = labels::Entity::find()
        .filter(labels::Column::ProjectId.eq(project_id))
        .all(&state.db)
        .await?;
    let items = list
        .into_iter()
        .map(|l| LabelExportItem {
            name: l.name,
            color: l.color,
            description: l.description,
        })
        .collect();
    Ok(Json(LabelExport {
        version: 1,
        labels: items,
    }))
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/import",
    tag = "Labels",
    summary = "ラベルを JSON インポート",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
    ),
    request_body = ImportLabelRequest,
    responses(
        (status = 200, description = "インポート後のラベル一覧", body = [LabelResponse]),
        CrudErrors,
    )
)]
pub async fn import_labels(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id)): Path<(Uuid, Uuid)>,
    Valid(Json(payload)): Valid<Json<ImportLabelRequest>>,
) -> Result<Json<Vec<LabelResponse>>, AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;

    let txn = state.db.begin().await?;
    for item in &payload.labels {
        item.validate().map_err(|_| AppError::BadRequest)?;
        let existing = labels::Entity::find()
            .filter(labels::Column::ProjectId.eq(project_id))
            .filter(labels::Column::Name.eq(&item.name))
            .one(&txn)
            .await?;

        match existing {
            Some(l) => {
                if matches!(payload.on_conflict, ImportConflict::Overwrite) {
                    let mut active: labels::ActiveModel = l.into();
                    active.color = Set(item.color.clone());
                    active.description = Set(item.description.clone());
                    active.update(&txn).await?;
                }
            }
            None => {
                labels::ActiveModel {
                    id: Set(Uuid::new_v4()),
                    name: Set(item.name.clone()),
                    description: Set(item.description.clone()),
                    color: Set(item.color.clone()),
                    icon_url: Set(None),
                    project_id: Set(Some(project_id)),
                }
                .insert(&txn)
                .await?;
            }
        }
    }
    txn.commit().await?;

    let list = labels::Entity::find()
        .filter(labels::Column::ProjectId.eq(project_id))
        .all(&state.db)
        .await?;
    Ok(Json(list.into_iter().map(Into::into).collect()))
}
