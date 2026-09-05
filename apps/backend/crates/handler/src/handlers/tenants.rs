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
use std::collections::HashSet;

use crate::auth_helpers::{guest_tenant_ids, is_tenant_member};
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
        (status = 200, description = "テナント一覧", body = [TenantListItemResponse]),
        CrudErrors,
    )
)]
pub async fn list_tenants(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<TenantListItemResponse>>, AppError> {
    // テナント一覧は ensure_tenant_owner/access 不要。
    // Session: 自分が所有するテナント + テナントメンバーとして参加しているテナント
    //          + project-only の客分として関わるテナント（membership=Guest の印付き）。
    //          has_tenant_access が許可する経路と同じ条件で抽出する
    //          （ここが owner/member だけだと、客分は入れるプロジェクトがあるのに一覧に出ない）。
    // PAT: バインドされた tenant_id のうち、所属（客分を含む）しているものだけ返す。
    // フィルタ自体が認可を兼ねているため追加チェックは不要。
    // 客分に tenant-wide の口は開かないため、クライアントは membership の印で
    // 開ける口を見分ける（apps/backend/docs/tenant-project-authz.md）。
    auth.require_scope(Scope::AdminTenant)?;
    let items = match &auth.method {
        AuthMethod::Session => {
            let joined_tenant_ids: HashSet<Uuid> = tenant_members::Entity::find()
                .filter(tenant_members::Column::UserId.eq(auth.user_id))
                .select_only()
                .column(tenant_members::Column::TenantId)
                .into_tuple::<Uuid>()
                .all(&state.db)
                .await?
                .into_iter()
                .collect();
            let guest_ids = guest_tenant_ids(&state.db, auth.user_id).await?;

            tenants::Entity::find()
                .filter(
                    Condition::any()
                        .add(tenants::Column::OwnerId.eq(auth.user_id))
                        .add(
                            tenants::Column::Id.is_in(
                                joined_tenant_ids
                                    .iter()
                                    .chain(guest_ids.iter())
                                    .copied()
                                    .collect::<Vec<_>>(),
                            ),
                        ),
                )
                .all(&state.db)
                .await?
                .into_iter()
                .map(|tenant| {
                    let membership = if tenant.owner_id == auth.user_id {
                        TenantMembershipKind::Owner
                    } else if joined_tenant_ids.contains(&tenant.id) {
                        TenantMembershipKind::Member
                    } else {
                        TenantMembershipKind::Guest
                    };
                    TenantListItemResponse::from_parts(tenant, membership)
                })
                .collect()
        }
        AuthMethod::PersonalToken { tenant_id, .. } => {
            // PAT はバインドされた単一テナントのみ返す。
            // バインドは「どのテナントを触れるか」の制限であって所属の証明ではないので、
            // テナントから外した利用者のトークンでは一覧にも出さない（`get_tenant` と同じ判定）。
            // ただし project-only の客分は、名指しのプロジェクトに入れる分だけ印付きで出す
            let mut visible = Vec::new();
            if let Some(tenant) = tenants::Entity::find_by_id(*tenant_id)
                .one(&state.db)
                .await?
            {
                let membership = if tenant.owner_id == auth.user_id {
                    Some(TenantMembershipKind::Owner)
                } else if is_tenant_member(&state.db, *tenant_id, auth.user_id).await? {
                    Some(TenantMembershipKind::Member)
                } else if guest_tenant_ids(&state.db, auth.user_id)
                    .await?
                    .contains(tenant_id)
                {
                    Some(TenantMembershipKind::Guest)
                } else {
                    None
                };
                if let Some(membership) = membership {
                    visible.push(TenantListItemResponse::from_parts(tenant, membership));
                }
            }
            visible
        }
    };
    Ok(Json(items))
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
