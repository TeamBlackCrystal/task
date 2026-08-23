use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use axum_valid::Valid;
use sea_orm::prelude::Uuid;
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder,
};

use crate::AppState;
use crate::error::AppError;
use crate::extractors::AuthUser;
use crate::openapi::{CrudErrors, SessionAuthErrors};
use entity::scopes::ScopeList;
use entity::{
    personal_tokens::{self},
    projects, tenants,
};
use payload::personal_tokens::*;
use service::auth;

fn token_last_four(token: &str) -> String {
    token[token.len().saturating_sub(4)..].to_string()
}

async fn require_tenant_owner(
    state: &AppState,
    tenant_id: Uuid,
    user_id: Uuid,
) -> Result<(), AppError> {
    let tenant = tenants::Entity::find_by_id(tenant_id)
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    if tenant.owner_id != user_id {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

async fn validate_project_ids(
    state: &AppState,
    tenant_id: Uuid,
    project_ids: &[Uuid],
) -> Result<(), AppError> {
    if project_ids.is_empty() {
        return Err(AppError::BadRequest);
    }

    let unique_ids: std::collections::HashSet<Uuid> = project_ids.iter().copied().collect();

    let count = projects::Entity::find()
        .filter(projects::Column::TenantId.eq(tenant_id))
        .filter(projects::Column::Id.is_in(unique_ids.iter().copied().collect::<Vec<_>>()))
        .count(&state.db)
        .await?;

    if count != unique_ids.len() as u64 {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

async fn get_owned_token(
    state: &AppState,
    token_id: Uuid,
    user_id: Uuid,
) -> Result<personal_tokens::Model, AppError> {
    personal_tokens::Entity::find_by_id(token_id)
        .filter(personal_tokens::Column::UserId.eq(user_id))
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/",
    tag = "Personal Tokens",
    summary = "自分のパーソナルアクセストークンの一覧",
    responses(
        (
            status = 200,
            description = "取り消し済みを除く、自分が発行したトークンの一覧",
            body = Vec<PersonalTokenResponse>
        ),
        SessionAuthErrors,
    )
)]
pub async fn list_personal_tokens(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<PersonalTokenResponse>>, AppError> {
    auth.require_session()?;

    let tokens = personal_tokens::Entity::find()
        .filter(personal_tokens::Column::UserId.eq(auth.user_id))
        .filter(personal_tokens::Column::Revoked.eq(false))
        .order_by_asc(personal_tokens::Column::Name)
        .order_by_asc(personal_tokens::Column::Id)
        .all(&state.db)
        .await?;

    let resp = tokens
        .into_iter()
        .map(PersonalTokenResponse::try_from)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| AppError::Internal(e.into()))?;
    Ok(Json(resp))
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/",
    tag = "Personal Tokens",
    summary = "パーソナルアクセストークンを発行",
    request_body = CreatePersonalTokenRequest,
    responses(
        (
            status = 201,
            description = "発行したトークンの情報（平文トークンはこの応答でのみ返却）",
            body = CreatePersonalTokenResponse
        ),
        SessionAuthErrors,
    )
)]
pub async fn create_personal_token(
    State(state): State<AppState>,
    auth: AuthUser,
    Valid(Json(payload)): Valid<Json<CreatePersonalTokenRequest>>,
) -> Result<(StatusCode, Json<CreatePersonalTokenResponse>), AppError> {
    auth.require_session()?;
    require_tenant_owner(&state, payload.tenant_id, auth.user_id).await?;

    if let Some(ref project_ids) = payload.project_ids {
        validate_project_ids(&state, payload.tenant_id, project_ids).await?;
    }

    let secret = &state.settings.personal_token_secret;
    let (token_value, token_hash) =
        auth::generate_personal_token(secret).map_err(|e| AppError::Internal(e.into()))?;

    let allowed_project_ids = payload.project_ids.map(|ids| serde_json::json!(ids));

    let model = personal_tokens::ActiveModel {
        id: Set(Uuid::new_v4()),
        name: Set(payload.name),
        token_last_four: Set(token_last_four(&token_value)),
        token_hash: Set(token_hash),
        expires_at: Set(payload.expires_at.map(Into::into)),
        last_used_at: Set(None),
        revoked: Set(false),
        user_id: Set(auth.user_id),
        scopes: Set(ScopeList(payload.scopes)),
        tenant_id: Set(payload.tenant_id),
        allowed_project_ids: Set(allowed_project_ids),
    }
    .insert(&state.db)
    .await?;

    let resp = CreatePersonalTokenResponse::new(token_value, model)
        .map_err(|e| AppError::Internal(e.into()))?;
    Ok((StatusCode::CREATED, Json(resp)))
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "Personal Tokens",
    summary = "指定したトークンを参照",
    params(("id" = Uuid, Path, description = "トークンの識別子")),
    responses(
        (
            status = 200,
            description = "トークンの状態",
            body = PersonalTokenResponse
        ),
        CrudErrors,
    )
)]
pub async fn get_personal_token(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<PersonalTokenResponse>, AppError> {
    auth.require_session()?;
    let token = get_owned_token(&state, id, auth.user_id).await?;
    let resp = PersonalTokenResponse::try_from(token).map_err(|e| AppError::Internal(e.into()))?;
    Ok(Json(resp))
}

#[axum::debug_handler]
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "Personal Tokens",
    summary = "指定したトークンを取り消し",
    params(("id" = Uuid, Path, description = "トークンの識別子")),
    responses(
        (status = 204, description = "取り消しました"),
        CrudErrors,
    )
)]
pub async fn revoke_personal_token(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    auth.require_session()?;
    let token = get_owned_token(&state, id, auth.user_id).await?;

    if token.revoked {
        return Ok(StatusCode::NO_CONTENT);
    }

    let mut active: personal_tokens::ActiveModel = token.into();
    active.revoked = Set(true);
    active.update(&state.db).await?;

    Ok(StatusCode::NO_CONTENT)
}

#[axum::debug_handler]
#[utoipa::path(
    delete,
    path = "/revoke-all",
    tag = "Personal Tokens",
    summary = "テナント配下のすべての個人用トークンを取り消し",
    request_body = RevokeAllPersonalTokensRequest,
    responses(
        (status = 204, description = "取り消しました"),
        CrudErrors,
    )
)]
pub async fn revoke_all_personal_tokens(
    State(state): State<AppState>,
    auth: AuthUser,
    Valid(Json(payload)): Valid<Json<RevokeAllPersonalTokensRequest>>,
) -> Result<StatusCode, AppError> {
    auth.require_session()?;
    require_tenant_owner(&state, payload.confirm_tenant_id, auth.user_id).await?;

    personal_tokens::Entity::update_many()
        .col_expr(personal_tokens::Column::Revoked, Expr::value(true))
        .filter(personal_tokens::Column::TenantId.eq(payload.confirm_tenant_id))
        .filter(personal_tokens::Column::Revoked.eq(false))
        .exec(&state.db)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}
