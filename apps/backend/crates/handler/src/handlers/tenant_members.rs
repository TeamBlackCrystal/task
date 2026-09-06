use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use axum_valid::Valid;
use sea_orm::prelude::Uuid;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, TransactionTrait,
};

use crate::AppState;
use crate::auth_helpers::is_tenant_owner;
use crate::error::{AppError, ServerError};
use crate::extractors::AuthUser;
use crate::handlers::project_members;
use crate::openapi::CrudErrors;
use entity::tenant_members::TenantRole;
use entity::{
    project_members as project_members_entity, projects, scopes::Scope, tenant_members, tenants,
    users,
};
use payload::tenant_members::*;
use service::access::visible_project_ids;
use service::db::is_postgres_unique_violation;

async fn ensure_tenant_exists(state: &AppState, tenant_id: Uuid) -> Result<(), AppError> {
    tenants::Entity::find_by_id(tenant_id)
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(())
}

/// メンバーの追加・変更・削除はオーナーとテナント Admin だけに許す。
pub(crate) async fn require_tenant_admin(
    state: &AppState,
    tenant_id: Uuid,
    user_id: Uuid,
) -> Result<(), AppError> {
    ensure_tenant_exists(state, tenant_id).await?;
    if is_tenant_owner(&state.db, tenant_id, user_id).await? {
        return Ok(());
    }
    let member = tenant_members::Entity::find()
        .filter(tenant_members::Column::TenantId.eq(tenant_id))
        .filter(tenant_members::Column::UserId.eq(user_id))
        .one(&state.db)
        .await?;
    match member {
        Some(m) if m.role == TenantRole::Admin => Ok(()),
        _ => Err(AppError::Forbidden),
    }
}

/// メンバー行に表示用のユーザー情報を同梱する。FK があるため利用者は必ず居るはずで、
/// 居なければ握り潰さず 500 にする。
async fn attach_users(
    state: &AppState,
    members: Vec<tenant_members::Model>,
) -> Result<Vec<TenantMemberResponse>, AppError> {
    let user_ids: Vec<Uuid> = members.iter().map(|m| m.user_id).collect();
    let mut users_by_id: std::collections::HashMap<Uuid, users::Model> = users::Entity::find()
        .filter(users::Column::Id.is_in(user_ids))
        .all(&state.db)
        .await?
        .into_iter()
        .map(|u| (u.id, u))
        .collect();
    members
        .into_iter()
        .map(|m| {
            let user = users_by_id
                .remove(&m.user_id)
                .ok_or_else(|| anyhow::anyhow!("tenant member {} has no user row", m.user_id))?;
            Ok(TenantMemberResponse::from_parts(m, user))
        })
        .collect()
}

async fn find_member(
    state: &AppState,
    tenant_id: Uuid,
    user_id: Uuid,
) -> Result<tenant_members::Model, AppError> {
    tenant_members::Entity::find()
        .filter(tenant_members::Column::TenantId.eq(tenant_id))
        .filter(tenant_members::Column::UserId.eq(user_id))
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/",
    tag = "Tenant Members",
    summary = "テナントメンバー一覧",
    params(("tenant_id" = Uuid, Path, description = "テナントID")),
    responses(
        (status = 200, description = "メンバー一覧", body = [TenantMemberResponse]),
        CrudErrors,
    )
)]
pub async fn list_members(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(tenant_id): Path<Uuid>,
) -> Result<Json<Vec<TenantMemberResponse>>, AppError> {
    auth.require_scope(Scope::AdminTenant)?;
    // 追加・変更・削除と違い、一覧の閲覧はテナントに入れる人なら誰でも許す
    // （PAT はテナント系エンドポイント共通で AdminTenant スコープを要求する）
    auth.ensure_tenant_access(&state, tenant_id, None).await?;

    let members = tenant_members::Entity::find()
        .filter(tenant_members::Column::TenantId.eq(tenant_id))
        .all(&state.db)
        .await?;
    Ok(Json(attach_users(&state, members).await?))
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/",
    tag = "Tenant Members",
    summary = "テナントメンバーを追加",
    params(("tenant_id" = Uuid, Path, description = "テナントID")),
    request_body = AddTenantMemberRequest,
    responses(
        (status = 201, description = "追加されたメンバー", body = TenantMemberResponse),
        (status = 409, description = "既にメンバーとして登録済み", body = ServerError),
        CrudErrors,
    )
)]
pub async fn add_member(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(tenant_id): Path<Uuid>,
    Valid(Json(payload)): Valid<Json<AddTenantMemberRequest>>,
) -> Result<(StatusCode, Json<TenantMemberResponse>), AppError> {
    auth.require_scope(Scope::AdminTenant)?;
    auth.ensure_tenant_access(&state, tenant_id, None).await?;
    require_tenant_admin(&state, tenant_id, auth.user_id).await?;

    let user = users::Entity::find_by_id(payload.user_id)
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    // 二重追加は UNIQUE (tenant_id, user_id) に任せる。
    // 事前に引いてから INSERT すると、同時に 2 回追加されたときに 500 になる
    let member = match (tenant_members::ActiveModel {
        id: Set(Uuid::new_v4()),
        tenant_id: Set(tenant_id),
        user_id: Set(payload.user_id),
        role: Set(payload.role),
    })
    .insert(&state.db)
    .await
    {
        Ok(member) => member,
        Err(e) if is_postgres_unique_violation(&e) => return Err(AppError::Conflict),
        Err(e) => return Err(e.into()),
    };

    Ok((
        StatusCode::CREATED,
        Json(TenantMemberResponse::from_parts(member, user)),
    ))
}

#[axum::debug_handler]
#[utoipa::path(
    put,
    path = "/{user_id}",
    tag = "Tenant Members",
    summary = "テナントメンバーの権限を変更",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("user_id" = Uuid, Path, description = "ユーザーID"),
    ),
    request_body = UpdateTenantMemberRequest,
    responses(
        (status = 200, description = "変更後のメンバー", body = TenantMemberResponse),
        CrudErrors,
    )
)]
pub async fn update_member(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, user_id)): Path<(Uuid, Uuid)>,
    Valid(Json(payload)): Valid<Json<UpdateTenantMemberRequest>>,
) -> Result<Json<TenantMemberResponse>, AppError> {
    auth.require_scope(Scope::AdminTenant)?;
    auth.ensure_tenant_access(&state, tenant_id, None).await?;
    require_tenant_admin(&state, tenant_id, auth.user_id).await?;

    let member = find_member(&state, tenant_id, user_id).await?;
    let user = users::Entity::find_by_id(user_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("tenant member {user_id} has no user row"))?;
    let mut active: tenant_members::ActiveModel = member.into();
    active.role = Set(payload.role);
    let updated = active.update(&state.db).await?;
    Ok(Json(TenantMemberResponse::from_parts(updated, user)))
}

#[axum::debug_handler]
#[utoipa::path(
    delete,
    path = "/{user_id}",
    tag = "Tenant Members",
    summary = "テナントメンバーを削除",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("user_id" = Uuid, Path, description = "ユーザーID"),
    ),
    responses(
        (status = 204, description = "削除しました"),
        CrudErrors,
    )
)]
pub async fn remove_member(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    auth.require_scope(Scope::AdminTenant)?;
    auth.ensure_tenant_access(&state, tenant_id, None).await?;
    require_tenant_admin(&state, tenant_id, auth.user_id).await?;

    let member = find_member(&state, tenant_id, user_id).await?;

    // プロジェクト側の「最後の Admin を残す」判定は、テナントに在籍している Admin だけを
    // 数える（`project_members::would_drop_last_admin`）。除名はその数え方の入力を変えるので、
    // 判定と同じロックの内側で行う。外すと、降格が「まだ B が居る」を読んだ後・書く前に
    // B を除名でき、降格が消えた相手を数えたまま通る。
    //
    // ここで Admin 数を数え直して 409 にはしない。除名は「この人はもう居ない」という
    // 宣言で、それをプロジェクトのロールで止めると、対象が単独 Admin のプロジェクトを
    // 全部直すまでオフボーディングできなくなる。Admin が全員抜けたプロジェクトは
    // テナントオーナーが直せる（`project_members::require_project_admin` は
    // オーナーを無条件で通す）。詳細は `lock_membership_changes` の doc を見る
    let txn = state.db.begin().await?;
    project_members::lock_membership_changes(&txn, tenant_id).await?;

    // 除名が消すのはテナント所属（継承）だけ。`project_members` の行はプロジェクト単位の
    // 明示 ACE であり、NTFS / NFSv4 の ACL と同じく継承元と独立して残る
    // （apps/backend/docs/tenant-project-authz.md の「継承と明示」）。残った行の持ち主は
    // project-only の客分になり、名指しされたプロジェクトの中だけ引き続き入れる。
    // 副次的に、その人しか指定されていなかったプロジェクトがメンバー 0 人になって
    // 「メンバー未指定＝テナント全体に開放」へ戻ることも防ぎ、再参加時は元の割り当てが戻る。
    // 残る明示 ACE は `list_explicit_projects` で除名の前後どちらでも確かめられる。
    // 通知の宛先はテナントに居る人だけに絞られる（`service::access`。客分には飛ばない）
    tenant_members::Entity::delete_by_id(member.id)
        .exec(&txn)
        .await?;
    txn.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/{user_id}/explicit-projects",
    tag = "Tenant Members",
    summary = "テナントメンバーが持つプロジェクト単位の明示 ACE",
    description = "テナント除名の前に「除名しても、この人はこれらのプロジェクトに入り続ける」と確かめるための口。除名の後に呼んでも同じ内容を返す。",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("user_id" = Uuid, Path, description = "ユーザーID"),
    ),
    responses(
        (status = 200, description = "明示 ACE の一覧", body = ExplicitProjectsResponse),
        CrudErrors,
    )
)]
pub async fn list_explicit_projects(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ExplicitProjectsResponse>, AppError> {
    auth.require_scope(Scope::AdminTenant)?;
    auth.ensure_tenant_access(&state, tenant_id, None).await?;
    // 除名と同じ相手（オーナーとテナント Admin）にだけ見せる。
    // 対象がテナントに居るかは問わない。除名した後に「まだ何が残っているか」を確かめる用途があるため
    require_tenant_admin(&state, tenant_id, auth.user_id).await?;

    let rows = project_members_entity::Entity::find()
        .filter(project_members_entity::Column::UserId.eq(user_id))
        .find_also_related(projects::Entity)
        .all(&state.db)
        .await?;
    let mut aces: Vec<(project_members_entity::Model, projects::Model)> = rows
        .into_iter()
        .filter_map(|(m, p)| p.filter(|p| p.tenant_id == tenant_id).map(|p| (m, p)))
        .collect();

    // 非公開プロジェクトの名前や key を、そのプロジェクトに入れない人へ出さない。
    // テナント Admin でもプロジェクトの閲覧は `list_projects` と同じ境界で絞られるので、
    // ここも同じ判定（`visible_project_ids`）を通す。オーナーは全プロジェクトに入れる
    let visible = if is_tenant_owner(&state.db, tenant_id, auth.user_id).await? {
        aces.iter().map(|(_, p)| p.id).collect()
    } else {
        visible_project_ids(
            &state.db,
            aces.iter().map(|(_, p)| p.id).collect(),
            auth.user_id,
        )
        .await?
    };
    let hidden_count = aces
        .iter()
        .filter(|(_, p)| !visible.contains(&p.id))
        .count() as u64;
    aces.retain(|(_, p)| visible.contains(&p.id));
    aces.sort_by(|a, b| a.1.key.cmp(&b.1.key));

    Ok(Json(ExplicitProjectsResponse {
        explicit_projects: aces
            .into_iter()
            .map(|(m, p)| ExplicitProjectAce {
                project_id: p.id,
                key: p.key,
                name: p.name,
                role: m.role,
            })
            .collect(),
        hidden_count,
    }))
}
