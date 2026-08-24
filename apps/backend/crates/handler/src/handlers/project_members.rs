use std::collections::{HashMap, HashSet};

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use axum_valid::Valid;
use sea_orm::prelude::Uuid;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QuerySelect,
};

use crate::AppState;
use crate::auth_helpers::{is_tenant_member, is_tenant_owner};
use crate::error::{AppError, ServerError};
use crate::extractors::AuthUser;
use crate::openapi::CrudErrors;
use entity::project_members::ProjectRole;
use entity::{
    project_members, projects, scopes::Scope, task_watchers, tasks, tenant_members, tenants, users,
};
use payload::project_members::*;

async fn get_project_in_tenant(
    state: &AppState,
    tenant_id: Uuid,
    project_id: Uuid,
) -> Result<projects::Model, AppError> {
    projects::Entity::find_by_id(project_id)
        .filter(projects::Column::TenantId.eq(tenant_id))
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)
}

pub(crate) async fn require_project_admin(
    state: &AppState,
    tenant_id: Uuid,
    project_id: Uuid,
    user_id: Uuid,
) -> Result<(), AppError> {
    let _ = get_project_in_tenant(state, tenant_id, project_id).await?;
    if is_tenant_owner(&state.db, tenant_id, user_id).await? {
        return Ok(());
    }
    let member = project_members::Entity::find()
        .filter(project_members::Column::ProjectId.eq(project_id))
        .filter(project_members::Column::UserId.eq(user_id))
        .one(&state.db)
        .await?;
    match member {
        Some(m) if m.role == ProjectRole::Admin => Ok(()),
        _ => Err(AppError::Forbidden),
    }
}

/// メンバー行に表示用のユーザー情報を同梱する。FK があるため利用者は必ず居るはずで、
/// 居なければ握り潰さず 500 にする。
async fn attach_users(
    state: &AppState,
    members: Vec<project_members::Model>,
) -> Result<Vec<ProjectMemberResponse>, AppError> {
    let user_ids: Vec<Uuid> = members.iter().map(|m| m.user_id).collect();
    let mut users_by_id: HashMap<Uuid, users::Model> = users::Entity::find()
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
                .ok_or_else(|| anyhow::anyhow!("project member {} has no user row", m.user_id))?;
            Ok(ProjectMemberResponse::from_parts(m, user))
        })
        .collect()
}

async fn find_member(
    state: &AppState,
    project_id: Uuid,
    user_id: Uuid,
) -> Result<project_members::Model, AppError> {
    project_members::Entity::find()
        .filter(project_members::Column::ProjectId.eq(project_id))
        .filter(project_members::Column::UserId.eq(user_id))
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)
}

/// 対象を Admin から外すと、テナントに残っている Admin が居なくなるか。
///
/// テナントから外した人の `project_members` の行はあえて残している
/// （`tenant_members::remove_member`）。単純に Admin の行数を数えると、
/// もう管理操作を実行できない人が最後の Admin 枠を占有し、その人を消すことも降格することも
/// 409 で弾かれてプロジェクトを直せなくなる。数えるのはテナントに残っている Admin だけにする。
async fn would_drop_last_admin(
    state: &AppState,
    tenant_id: Uuid,
    project_id: Uuid,
    target_user_id: Uuid,
) -> Result<bool, AppError> {
    let admin_ids: Vec<Uuid> = project_members::Entity::find()
        .filter(project_members::Column::ProjectId.eq(project_id))
        .filter(project_members::Column::Role.eq(ProjectRole::Admin))
        .select_only()
        .column(project_members::Column::UserId)
        .into_tuple()
        .all(&state.db)
        .await?;
    if admin_ids.is_empty() {
        return Ok(false);
    }

    // 在籍確認は 1 人ずつ引かずまとめて引く（Admin の人数分クエリを出さない）
    let owner_id = tenants::Entity::find_by_id(tenant_id)
        .one(&state.db)
        .await?
        .map(|t| t.owner_id);
    let member_ids: HashSet<Uuid> = tenant_members::Entity::find()
        .filter(tenant_members::Column::TenantId.eq(tenant_id))
        .filter(tenant_members::Column::UserId.is_in(admin_ids.clone()))
        .select_only()
        .column(tenant_members::Column::UserId)
        .into_tuple::<Uuid>()
        .all(&state.db)
        .await?
        .into_iter()
        .collect();

    let active = |user_id: Uuid| owner_id == Some(user_id) || member_ids.contains(&user_id);
    let target_is_active_admin = admin_ids.contains(&target_user_id) && active(target_user_id);
    let remaining = admin_ids
        .iter()
        .filter(|id| **id != target_user_id && active(**id))
        .count();
    Ok(target_is_active_admin && remaining == 0)
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/",
    operation_id = "list_project_members",
    tag = "Project Members",
    summary = "プロジェクトメンバー一覧",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
    ),
    responses(
        (status = 200, description = "メンバー一覧", body = [ProjectMemberResponse]),
        CrudErrors,
    )
)]
pub async fn list_members(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<ProjectMemberResponse>>, AppError> {
    auth.require_scope(Scope::ReadProject)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    require_project_admin(&state, tenant_id, project_id, auth.user_id).await?;
    let members = project_members::Entity::find()
        .filter(project_members::Column::ProjectId.eq(project_id))
        .all(&state.db)
        .await?;
    Ok(Json(attach_users(&state, members).await?))
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/",
    operation_id = "add_project_member",
    tag = "Project Members",
    summary = "プロジェクトメンバーを追加",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
    ),
    request_body = AddMemberRequest,
    responses(
        (status = 201, description = "追加されたメンバー", body = ProjectMemberResponse),
        (status = 400, description = "テナントメンバーでない利用者は追加できません", body = ServerError),
        (status = 409, description = "既にメンバーとして登録済み", body = ServerError),
        CrudErrors,
    )
)]
pub async fn add_member(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id)): Path<(Uuid, Uuid)>,
    Valid(Json(payload)): Valid<Json<AddMemberRequest>>,
) -> Result<(StatusCode, Json<ProjectMemberResponse>), AppError> {
    auth.require_scope(Scope::WriteProject)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    require_project_admin(&state, tenant_id, project_id, auth.user_id).await?;

    let user = users::Entity::find_by_id(payload.user_id)
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    // プロジェクトメンバーはテナントメンバーの絞り込みなので、テナントに居ない人は入れない。
    // ここを許すと「プロジェクトには居るがテナントには入れない」不整合な状態ができる（#568）
    if !is_tenant_owner(&state.db, tenant_id, payload.user_id).await?
        && !is_tenant_member(&state.db, tenant_id, payload.user_id).await?
    {
        return Err(AppError::BadRequest);
    }

    let existing = project_members::Entity::find()
        .filter(project_members::Column::ProjectId.eq(project_id))
        .filter(project_members::Column::UserId.eq(payload.user_id))
        .one(&state.db)
        .await?;
    if existing.is_some() {
        return Err(AppError::Conflict);
    }

    let member = project_members::ActiveModel {
        id: Set(Uuid::new_v4()),
        project_id: Set(project_id),
        user_id: Set(payload.user_id),
        role: Set(payload.role),
    };
    let model = member.insert(&state.db).await?;
    Ok((
        StatusCode::CREATED,
        Json(ProjectMemberResponse::from_parts(model, user)),
    ))
}

#[axum::debug_handler]
#[utoipa::path(
    put,
    path = "/{user_id}",
    operation_id = "update_project_member",
    tag = "Project Members",
    summary = "プロジェクトメンバーの権限を変更",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("user_id" = Uuid, Path, description = "ユーザーID"),
    ),
    request_body = UpdateMemberRequest,
    responses(
        (status = 200, description = "更新後のメンバー", body = ProjectMemberResponse),
        (status = 409, description = "最後のAdminは降格できません", body = ServerError),
        CrudErrors,
    )
)]
pub async fn update_member(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, member_user_id)): Path<(Uuid, Uuid, Uuid)>,
    Valid(Json(payload)): Valid<Json<UpdateMemberRequest>>,
) -> Result<Json<ProjectMemberResponse>, AppError> {
    auth.require_scope(Scope::WriteProject)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    require_project_admin(&state, tenant_id, project_id, auth.user_id).await?;
    let current = find_member(&state, project_id, member_user_id).await?;
    if payload.role != ProjectRole::Admin
        && would_drop_last_admin(&state, tenant_id, project_id, member_user_id).await?
    {
        return Err(AppError::Conflict);
    }
    let user = users::Entity::find_by_id(member_user_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("project member {member_user_id} has no user row"))?;
    let mut active: project_members::ActiveModel = current.into();
    active.role = Set(payload.role);
    let updated = active.update(&state.db).await?;
    Ok(Json(ProjectMemberResponse::from_parts(updated, user)))
}

#[axum::debug_handler]
#[utoipa::path(
    delete,
    path = "/{user_id}",
    operation_id = "remove_project_member",
    tag = "Project Members",
    summary = "プロジェクトメンバーを削除",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("user_id" = Uuid, Path, description = "ユーザーID"),
    ),
    responses(
        (status = 204, description = "削除しました"),
        (status = 409, description = "最後のAdminは削除できません", body = ServerError),
        CrudErrors,
    )
)]
pub async fn remove_member(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, member_user_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    auth.require_scope(Scope::WriteProject)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    require_project_admin(&state, tenant_id, project_id, auth.user_id).await?;
    let member = find_member(&state, project_id, member_user_id).await?;
    if would_drop_last_admin(&state, tenant_id, project_id, member_user_id).await? {
        return Err(AppError::Conflict);
    }
    // プロジェクト配下タスクの watcher を削除してから member を削除
    let task_ids: Vec<Uuid> = tasks::Entity::find()
        .select_only()
        .column(tasks::Column::Id)
        .filter(tasks::Column::ProjectId.eq(project_id))
        .into_tuple::<Uuid>()
        .all(&state.db)
        .await?;
    if !task_ids.is_empty() {
        task_watchers::Entity::delete_many()
            .filter(task_watchers::Column::UserId.eq(member_user_id))
            .filter(task_watchers::Column::TaskId.is_in(task_ids))
            .exec(&state.db)
            .await?;
    }
    project_members::Entity::delete_by_id(member.id)
        .exec(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
