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
use entity::project_custom_fields;
use payload::custom_fields::*;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    prelude::Uuid,
};
use service::custom_fields::validate_select_options;

async fn next_position(state: &AppState, project_id: Uuid) -> Result<i16, AppError> {
    let max = project_custom_fields::Entity::find()
        .filter(project_custom_fields::Column::ProjectId.eq(project_id))
        .order_by_desc(project_custom_fields::Column::Position)
        .one(&state.db)
        .await?
        .map(|f| f.position)
        .unwrap_or(-1);
    Ok(max.saturating_add(1))
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/",
    tag = "Custom Fields",
    summary = "プロジェクトのカスタムフィールド一覧",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
    ),
    responses(
        (status = 200, description = "カスタムフィールド一覧", body = CustomFieldListResponse),
        CrudErrors,
    )
)]
pub async fn list_custom_fields(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<CustomFieldListResponse>, AppError> {
    auth.require_scope(entity::scopes::Scope::ReadTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let fields = project_custom_fields::Entity::find()
        .filter(project_custom_fields::Column::ProjectId.eq(project_id))
        .order_by_asc(project_custom_fields::Column::Position)
        .order_by_asc(project_custom_fields::Column::CreatedAt)
        .all(&state.db)
        .await?;
    Ok(Json(CustomFieldListResponse {
        fields: fields.into_iter().map(Into::into).collect(),
    }))
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/",
    tag = "Custom Fields",
    summary = "カスタムフィールド作成",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
    ),
    request_body = CreateCustomFieldRequest,
    responses(
        (status = 201, description = "作成されたカスタムフィールド", body = ProjectCustomFieldResponse),
        CrudErrors,
    )
)]
pub async fn create_custom_field(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id)): Path<(Uuid, Uuid)>,
    Valid(Json(payload)): Valid<Json<CreateCustomFieldRequest>>,
) -> Result<(StatusCode, Json<ProjectCustomFieldResponse>), AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    if payload.field_type == project_custom_fields::CustomFieldType::Select {
        validate_select_options(&payload.options)?;
    } else if payload.options.is_some() {
        return Err(AppError::BadRequest);
    }
    let position = payload
        .position
        .unwrap_or(next_position(&state, project_id).await?);
    let field = project_custom_fields::ActiveModel {
        id: Set(Uuid::new_v4()),
        project_id: Set(project_id),
        name: Set(payload.name),
        field_type: Set(payload.field_type),
        options: Set(payload.options),
        is_required: Set(payload.is_required),
        position: Set(position),
        created_at: Set(chrono::Utc::now().into()),
    }
    .insert(&state.db)
    .await?;
    Ok((StatusCode::CREATED, Json(field.into())))
}

#[axum::debug_handler]
#[utoipa::path(
    patch,
    path = "/{field_id}",
    tag = "Custom Fields",
    summary = "カスタムフィールド更新",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("field_id" = Uuid, Path, description = "カスタムフィールドID"),
    ),
    request_body = UpdateCustomFieldRequest,
    responses(
        (status = 200, description = "更新後のカスタムフィールド", body = ProjectCustomFieldResponse),
        CrudErrors,
    )
)]
pub async fn update_custom_field(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, field_id)): Path<(Uuid, Uuid, Uuid)>,
    Valid(Json(payload)): Valid<Json<UpdateCustomFieldRequest>>,
) -> Result<Json<ProjectCustomFieldResponse>, AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let field = project_custom_fields::Entity::find_by_id(field_id)
        .filter(project_custom_fields::Column::ProjectId.eq(project_id))
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    if let Some(ref options) = payload.options {
        if field.field_type == project_custom_fields::CustomFieldType::Select {
            validate_select_options(&Some(options.clone()))?;
        } else {
            return Err(AppError::BadRequest);
        }
    }
    let mut active: project_custom_fields::ActiveModel = field.into();
    if let Some(name) = payload.name {
        active.name = Set(name);
    }
    if let Some(options) = payload.options {
        active.options = Set(Some(options));
    }
    if let Some(is_required) = payload.is_required {
        active.is_required = Set(is_required);
    }
    if let Some(position) = payload.position {
        active.position = Set(position);
    }
    Ok(Json(active.update(&state.db).await?.into()))
}

#[axum::debug_handler]
#[utoipa::path(
    delete,
    path = "/{field_id}",
    tag = "Custom Fields",
    summary = "カスタムフィールド削除",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("field_id" = Uuid, Path, description = "カスタムフィールドID"),
    ),
    responses(
        (status = 204, description = "削除しました"),
        CrudErrors,
    )
)]
pub async fn delete_custom_field(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, field_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let result = project_custom_fields::Entity::delete_many()
        .filter(project_custom_fields::Column::Id.eq(field_id))
        .filter(project_custom_fields::Column::ProjectId.eq(project_id))
        .exec(&state.db)
        .await?;
    if result.rows_affected == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}
