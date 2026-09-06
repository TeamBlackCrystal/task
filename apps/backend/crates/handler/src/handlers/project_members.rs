use std::collections::{HashMap, HashSet};

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use axum_valid::Valid;
use sea_orm::prelude::Uuid;
use sea_orm::sea_query::LockType;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect, TransactionTrait,
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
use payload::users::UserSummary;
use service::access::assignable_user_ids;

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/",
    operation_id = "list_assignable_users",
    tag = "Project Members",
    summary = "担当者に指定できる利用者一覧",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
    ),
    responses(
        (status = 200, description = "担当者候補", body = [UserSummary]),
        CrudErrors,
    )
)]
/// タスクの担当者に指定できる利用者。
///
/// メンバー一覧（`list_members`）とは別に置く。あちらはメンバー管理の口で
/// プロジェクト管理者しか読めないが、担当者の割り当ては `WriteTask` があれば
/// できるため、候補だけ取れずに 403 になる。返す集合も違う:
/// メンバーを 1 人も指定していない共有プロジェクトはテナント全体へ開放されるので、
/// `project_members` の行ではなく `assignable_user_ids` の判定で返す。
///
/// **スコープは割り当て API（`add_assignee` / `remove_assignee`）と同じ `WriteTask`。**
/// これは担当者を編集するための候補一覧で、読むだけの主体に見せる情報ではない。
/// `ReadTask` で通すと、read-only の PAT でも「そのプロジェクトで担当者にできる
/// 利用者全員」の名前とアイコン URL を列挙できてしまう。メンバー未指定の共有
/// プロジェクトではテナント全体が返るので、既存タスクを読むだけでは分からない
/// 利用者まで公開範囲が広がる。
pub async fn list_assignable_users(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<UserSummary>>, AppError> {
    auth.require_scope(Scope::WriteTask)?;
    // プロジェクト単位のアクセス可否はここで見る（担当者を触れる人が読める）
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;

    let ids = assignable_user_ids(&state.db, tenant_id, project_id).await?;
    if ids.is_empty() {
        return Ok(Json(Vec::new()));
    }
    let users = users::Entity::find()
        .filter(users::Column::Id.is_in(ids))
        .order_by_asc(users::Column::Username)
        .all(&state.db)
        .await?;
    Ok(Json(users.into_iter().map(UserSummary::from).collect()))
}

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

async fn find_member<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    user_id: Uuid,
) -> Result<project_members::Model, AppError> {
    project_members::Entity::find()
        .filter(project_members::Column::ProjectId.eq(project_id))
        .filter(project_members::Column::UserId.eq(user_id))
        .one(db)
        .await?
        .ok_or(AppError::NotFound)
}

/// 「最後の Admin を残す」判定に関わる書き込みを、テナント単位で直列化する。
///
/// 判定は数えてから書くので、掴まないと読みと書きの間に割り込まれる。
///
/// - 同じプロジェクトで A と B が同時に相手を Viewer へ落とすと、双方が
///   「まだもう 1 人いる」を読んで両方通る
/// - 降格が B を在籍中の Admin として数えている最中に B が除名されると、
///   降格は消えた相手を数えたまま通る
///
/// 掴むのはプロジェクト行ではなく**テナント行**。判定([`would_drop_last_admin`])が
/// 読むのは `project_members` と `tenant_members` の両方で、後者はプロジェクト行を
/// 掴んでも守れない。テナント行なら 1 つで両方を覆えるので、複数のロックと
/// その取得順序を持たずに済む。
///
/// この関数は [`crate::handlers::tenant_members::remove_member`] からも呼ぶ。
/// 片側だけが掴んでも直列化にならない。
///
/// # このロックが保証しないこと
///
/// **「在籍している Admin が常に 1 人以上いる」は保証しない。** 直列化しても
/// 「A を降格 →（別の操作として）B を除名」の順は両方とも正当で、結果として
/// 在籍 Admin は 0 人になりうる。除名を 409 で止めれば揃うが、対象が単独 Admin の
/// プロジェクトを全部直すまでテナントから外せなくなり、退職者のオフボーディングが
/// 止まる。`admin_users::delete_user_cascade`（強制削除）は 409 を返せないので、
/// どのみち同じ状態には到達する。
///
/// 実際に維持しているのは**「そのプロジェクトを管理できる人が常に居る」**で、
/// [`require_project_admin`] がテナントオーナーを無条件で通すことで成立している。
/// Admin が全員テナントから抜けた状態は想定内で、[`would_drop_last_admin`] が
/// 在籍者だけを数えるのはまさにそこから復旧できるようにするため。
///
/// 同じテナントの別プロジェクトのメンバー操作まで待たされるが、管理操作は
/// 頻度が低いので実害がない。
pub(crate) async fn lock_membership_changes<C: ConnectionTrait>(
    db: &C,
    tenant_id: Uuid,
) -> Result<(), AppError> {
    tenants::Entity::find_by_id(tenant_id)
        .lock(LockType::Update)
        .one(db)
        .await?;
    Ok(())
}

/// 対象を Admin から外すと、テナントに残っている Admin が居なくなるか。
///
/// テナントから外した人の `project_members` の行はあえて残している
/// （`tenant_members::remove_member`）。単純に Admin の行数を数えると、
/// もう管理操作を実行できない人が最後の Admin 枠を占有し、その人を消すことも降格することも
/// 409 で弾かれてプロジェクトを直せなくなる。数えるのはテナントに残っている Admin だけにする。
async fn would_drop_last_admin<C: ConnectionTrait>(
    db: &C,
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
        .all(db)
        .await?;
    if admin_ids.is_empty() {
        return Ok(false);
    }

    // 在籍確認は 1 人ずつ引かずまとめて引く（Admin の人数分クエリを出さない）
    let owner_id = tenants::Entity::find_by_id(tenant_id)
        .one(db)
        .await?
        .map(|t| t.owner_id);
    let member_ids: HashSet<Uuid> = tenant_members::Entity::find()
        .filter(tenant_members::Column::TenantId.eq(tenant_id))
        .filter(tenant_members::Column::UserId.is_in(admin_ids.clone()))
        .select_only()
        .column(tenant_members::Column::UserId)
        .into_tuple::<Uuid>()
        .all(db)
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

    // テナント所属の確認と insert を、テナント除名と同じロックの内側で行う。
    // 外すと、確認が「まだ居る」を読んだ後・書く前に除名が通り、除名済みの人へ
    // 明示 ACE（客分の口）を新しく作れてしまう。除名の側で「何が残るか」を確かめても、
    // その後に増えた行は見えない（`lock_membership_changes` の doc）
    let txn = state.db.begin().await?;
    lock_membership_changes(&txn, tenant_id).await?;

    // プロジェクトメンバーはテナントメンバーの絞り込みなので、テナントに居ない人は入れない。
    // ここを許すと「プロジェクトには居るがテナントには入れない」不整合な状態ができる（#568）
    if !is_tenant_owner(&state.db, tenant_id, payload.user_id).await?
        && !is_tenant_member(&txn, tenant_id, payload.user_id).await?
    {
        return Err(AppError::BadRequest);
    }

    let existing = project_members::Entity::find()
        .filter(project_members::Column::ProjectId.eq(project_id))
        .filter(project_members::Column::UserId.eq(payload.user_id))
        .one(&txn)
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
    let model = member.insert(&txn).await?;
    txn.commit().await?;
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
    // 数えてから書くので、同じプロジェクトのメンバー変更を直列化する
    let txn = state.db.begin().await?;
    lock_membership_changes(&txn, tenant_id).await?;
    let current = find_member(&txn, project_id, member_user_id).await?;
    if payload.role != ProjectRole::Admin
        && would_drop_last_admin(&txn, tenant_id, project_id, member_user_id).await?
    {
        return Err(AppError::Conflict);
    }
    let user = users::Entity::find_by_id(member_user_id)
        .one(&txn)
        .await?
        .ok_or_else(|| anyhow::anyhow!("project member {member_user_id} has no user row"))?;
    let mut active: project_members::ActiveModel = current.into();
    active.role = Set(payload.role);
    let updated = active.update(&txn).await?;
    txn.commit().await?;
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
    // 数えてから書くので、同じプロジェクトのメンバー変更を直列化する
    let txn = state.db.begin().await?;
    lock_membership_changes(&txn, tenant_id).await?;
    let member = find_member(&txn, project_id, member_user_id).await?;
    if would_drop_last_admin(&txn, tenant_id, project_id, member_user_id).await? {
        return Err(AppError::Conflict);
    }
    // プロジェクト配下タスクの watcher を削除してから member を削除
    let task_ids: Vec<Uuid> = tasks::Entity::find()
        .select_only()
        .column(tasks::Column::Id)
        .filter(tasks::Column::ProjectId.eq(project_id))
        .into_tuple::<Uuid>()
        .all(&txn)
        .await?;
    if !task_ids.is_empty() {
        task_watchers::Entity::delete_many()
            .filter(task_watchers::Column::UserId.eq(member_user_id))
            .filter(task_watchers::Column::TaskId.is_in(task_ids))
            .exec(&txn)
            .await?;
    }
    project_members::Entity::delete_by_id(member.id)
        .exec(&txn)
        .await?;
    txn.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
