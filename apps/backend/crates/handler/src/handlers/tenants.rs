use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use axum_valid::Valid;
use sea_orm::prelude::Uuid;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, Condition, EntityTrait, QueryFilter,
    QuerySelect,
};

use crate::AppState;
use crate::auth_helpers::is_tenant_member;
use crate::error::AppError;
use crate::extractors::AuthMethod;
use crate::extractors::AuthUser;
use crate::openapi::{CrudErrors, TenantCreateErrors};
use entity::{scopes::Scope, tenant_members, tenants};
use payload::tenants::*;

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/",
    tag = "Tenants",
    summary = "テナントを作成",
    request_body = CreateTenantRequest,
    responses(
        (status = 201, description = "作成されたテナント", body = TenantResponse),
        TenantCreateErrors,
    )
)]
pub async fn create_tenant(
    State(state): State<AppState>,
    auth: AuthUser,
    Valid(Json(payload)): Valid<Json<CreateTenantRequest>>,
) -> Result<(StatusCode, Json<TenantResponse>), AppError> {
    // PAT はテナントにバインドされているため、新規テナント作成はセッション専用とする
    auth.require_session()?;
    let id = Uuid::new_v4();
    let tenant = tenants::ActiveModel {
        id: Set(id),
        display_id: Set(payload.display_id),
        name: Set(payload.name),
        description: Set(payload.description),
        icon_url: Set(payload.icon_url),
        // 作成時は選ばせない。未設定のまま返し、画面が既定の絵文字を出す
        icon_emoji: Set(None),
        owner_id: Set(auth.user_id),
        drive_quota_bytes: Set(None),
        require_2fa: Set(false),
    };
    let model = tenant.insert(&state.db).await?;
    Ok((StatusCode::CREATED, Json(model.into())))
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/",
    tag = "Tenants",
    summary = "自分のテナント一覧",
    responses(
        (status = 200, description = "テナント一覧", body = [TenantResponse]),
        CrudErrors,
    )
)]
pub async fn list_tenants(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<TenantResponse>>, AppError> {
    // テナント一覧は ensure_tenant_owner/access 不要。
    // Session: 自分が所有するテナント + テナントメンバーとして参加しているテナント。
    //          has_tenant_access が許可する経路と同じ条件で抽出する
    //          （ここが owner だけだと、参加者はアクセスできるのに一覧に出ない）。
    // PAT: バインドされた tenant_id のうち、所属しているものだけ返す。
    // フィルタ自体が認可を兼ねているため追加チェックは不要。
    auth.require_scope(Scope::AdminTenant)?;
    let tenants = match &auth.method {
        AuthMethod::Session => {
            let joined_tenant_ids: Vec<Uuid> = tenant_members::Entity::find()
                .filter(tenant_members::Column::UserId.eq(auth.user_id))
                .select_only()
                .column(tenant_members::Column::TenantId)
                .into_tuple()
                .all(&state.db)
                .await?;

            tenants::Entity::find()
                .filter(
                    Condition::any()
                        .add(tenants::Column::OwnerId.eq(auth.user_id))
                        .add(tenants::Column::Id.is_in(joined_tenant_ids)),
                )
                .all(&state.db)
                .await?
        }
        AuthMethod::PersonalToken { tenant_id, .. } => {
            // PAT はバインドされた単一テナントのみ返す。
            // ただしバインドは「どのテナントを触れるか」の制限であって所属の証明ではないので、
            // テナントから外した利用者のトークンでは一覧にも出さない（`get_tenant` と同じ判定）
            let mut visible = Vec::new();
            if let Some(tenant) = tenants::Entity::find_by_id(*tenant_id)
                .one(&state.db)
                .await?
                && (tenant.owner_id == auth.user_id
                    || is_tenant_member(&state.db, *tenant_id, auth.user_id).await?)
            {
                visible.push(tenant);
            }
            visible
        }
    };
    Ok(Json(tenants.into_iter().map(Into::into).collect()))
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "Tenants",
    summary = "テナントを取得",
    params(("id" = Uuid, Path, description = "テナントID")),
    responses(
        (status = 200, description = "テナント情報", body = TenantResponse),
        CrudErrors,
    )
)]
pub async fn get_tenant(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<TenantResponse>, AppError> {
    // テナント情報の取得はメンバーにも許す。ここをオーナー専用にすると
    // 一覧に出るのに開けないテナントができてしまう（設定変更・削除は別途オーナー専用）。
    auth.require_scope(Scope::AdminTenant)?;
    auth.ensure_tenant_access(&state, id, None).await?;
    let tenant = tenants::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(tenant.into()))
}

#[axum::debug_handler]
#[utoipa::path(
    put,
    path = "/{id}",
    tag = "Tenants",
    summary = "テナントを更新",
    params(("id" = Uuid, Path, description = "テナントID")),
    request_body = UpdateTenantRequest,
    responses(
        (status = 200, description = "更新後のテナント", body = TenantResponse),
        CrudErrors,
    )
)]
pub async fn update_tenant(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Valid(Json(payload)): Valid<Json<UpdateTenantRequest>>,
) -> Result<Json<TenantResponse>, AppError> {
    // テナント設定の変更はオーナー専用操作。
    // ensure_tenant_access ではなく ensure_tenant_owner を使い、
    // プロジェクトメンバーによる誤操作を防ぐ。
    auth.require_scope(Scope::AdminTenant)?;
    let tenant = auth.ensure_tenant_owner(&state, id).await?;

    let mut active: tenants::ActiveModel = tenant.into();
    if let Some(name) = payload.name {
        active.name = Set(name);
    }
    if let Some(description) = payload.description {
        active.description = Set(description);
    }
    if let Some(icon_url) = payload.icon_url {
        active.icon_url = Set(icon_url);
    }
    if let Some(icon_emoji) = payload.icon_emoji {
        active.icon_emoji = Set(Some(icon_emoji));
    }
    let updated = active.update(&state.db).await?;
    Ok(Json(updated.into()))
}

#[axum::debug_handler]
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "Tenants",
    summary = "テナントを削除",
    params(("id" = Uuid, Path, description = "テナントID")),
    responses(
        (status = 204, description = "削除しました"),
        CrudErrors,
    )
)]
pub async fn delete_tenant(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    // テナント削除はオーナー専用操作。
    // ensure_tenant_access ではなく ensure_tenant_owner を使い、
    // プロジェクトメンバーによる削除を防ぐ。
    auth.require_scope(Scope::AdminTenant)?;
    auth.ensure_tenant_owner(&state, id).await?;
    tenants::Entity::delete_by_id(id).exec(&state.db).await?;
    Ok(StatusCode::NO_CONTENT)
}
