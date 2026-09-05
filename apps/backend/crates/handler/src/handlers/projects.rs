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
use crate::auth_helpers::{is_tenant_owner, visible_project_ids};
use crate::error::AppError;
use crate::extractors::AuthUser;
use crate::openapi::CrudErrors;
use entity::{drive_folders, project_statuses, projects, scopes::Scope};
use payload::projects::*;
use service::db::is_postgres_unique_violation;

fn generate_project_key(name: &str) -> String {
    let upper: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .take(10)
        .collect();
    if upper.is_empty() {
        "PROJ".to_string()
    } else {
        upper
    }
}

fn validate_project_key(key: &str) -> bool {
    let chars: Vec<char> = key.chars().collect();
    (chars.len() >= 2 && chars.len() <= 10)
        && chars[0].is_ascii_uppercase()
        && chars[1..]
            .iter()
            .all(|c| c.is_ascii_alphanumeric() && (c.is_ascii_uppercase() || c.is_ascii_digit()))
}

const INVALID_PROJECT_KEY_MESSAGE: &str = "key は 2〜10 文字で、先頭は大文字英字、残りは大文字英字または数字で入力してください（例: ENG, BACK）";

async fn require_tenant_owner(
    state: &AppState,
    tenant_id: Uuid,
    user_id: Uuid,
) -> Result<(), AppError> {
    if is_tenant_owner(&state.db, tenant_id, user_id).await? {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/",
    tag = "Projects",
    summary = "プロジェクトを作成",
    params(("tenant_id" = Uuid, Path, description = "テナントID")),
    request_body = CreateProjectRequest,
    responses(
        (status = 201, description = "作成されたプロジェクト", body = ProjectResponse),
        CrudErrors,
    )
)]
pub async fn create_project(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(tenant_id): Path<Uuid>,
    Valid(Json(payload)): Valid<Json<CreateProjectRequest>>,
) -> Result<(StatusCode, Json<ProjectResponse>), AppError> {
    auth.require_scope(Scope::WriteProject)?;
    auth.ensure_tenant_access(&state, tenant_id, None).await?;
    require_tenant_owner(&state, tenant_id, auth.user_id).await?;
    let explicit_key = payload.key;
    let mut key = match explicit_key.as_ref() {
        Some(k) if validate_project_key(k) => k.clone(),
        Some(_) => {
            return Err(AppError::BadRequestDetail(
                INVALID_PROJECT_KEY_MESSAGE.into(),
            ));
        }
        None => {
            let generated = generate_project_key(&payload.name);
            if validate_project_key(&generated) {
                generated
            } else {
                "PROJ".to_string()
            }
        }
    };
    let txn = state.db.begin().await?;
    let model = loop {
        let project = projects::ActiveModel {
            id: Set(Uuid::new_v4()),
            name: Set(payload.name.clone()),
            description: Set(payload.description.clone()),
            tenant_id: Set(tenant_id),
            icon_emoji: Set(payload.icon_emoji.clone()),
            icon_url: Set(payload.icon_url.clone()),
            key: Set(key.clone()),
            is_personal: Set(false),
            personal_owner_id: Set(None),
        };
        match project.insert(&txn).await {
            Ok(model) => break model,
            Err(e) if explicit_key.is_none() && is_postgres_unique_violation(&e) => {
                let suffix = Uuid::new_v4().simple().to_string().to_ascii_uppercase();
                let suffix = &suffix[..4];
                let max_base = 10usize.saturating_sub(suffix.len()).max(2);
                key = format!("{}{}", &key[..key.len().min(max_base)], suffix);
                if !validate_project_key(&key) {
                    return Err(AppError::Conflict);
                }
            }
            Err(e) => return Err(e.into()),
        }
    };

    let drive_folder = drive_folders::ActiveModel {
        id: Set(Uuid::new_v4()),
        name: Set(model.name.clone()),
        parent_id: Set(None),
        tenant_id: Set(tenant_id),
        project_id: Set(Some(model.id)),
        created_by: Set(auth.user_id),
        created_at: Set(chrono::Utc::now().into()),
    };
    drive_folder.insert(&txn).await?;
    seed_default_statuses(&txn, model.id).await?;
    txn.commit().await?;

    Ok((StatusCode::CREATED, Json(model.into())))
}

/// 新規プロジェクトに既定のワークフローステータスを seed する（#368）。
/// personal project 側（my_tasks の seed_personal_project_defaults）と異なり、
/// 通常プロジェクトはカンバン前提の 4 ステータスを用意する。
async fn seed_default_statuses(
    txn: &sea_orm::DatabaseTransaction,
    project_id: Uuid,
) -> Result<(), AppError> {
    let defaults: [(&str, &str, bool, bool); 4] = [
        ("Backlog", "#94a3b8", false, false),
        ("Todo", "#3b82f6", true, false),
        ("In Progress", "#f59e0b", false, false),
        ("Done", "#22c55e", false, true),
    ];
    for (position, (name, color, is_default, is_done_state)) in defaults.into_iter().enumerate() {
        project_statuses::ActiveModel {
            id: Set(Uuid::new_v4()),
            project_id: Set(project_id),
            name: Set(name.into()),
            color: Set(color.into()),
            position: Set(position as i16),
            is_default: Set(is_default),
            is_done_state: Set(is_done_state),
            created_at: Set(chrono::Utc::now().into()),
        }
        .insert(txn)
        .await?;
    }
    Ok(())
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/",
    tag = "Projects",
    summary = "プロジェクト一覧",
    params(("tenant_id" = Uuid, Path, description = "テナントID")),
    responses(
        (status = 200, description = "プロジェクト一覧", body = [ProjectResponse]),
        CrudErrors,
    )
)]
pub async fn list_projects(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(tenant_id): Path<Uuid>,
) -> Result<Json<Vec<ProjectResponse>>, AppError> {
    auth.require_scope(Scope::ReadProject)?;
    // project-only の客分には、明示 member の project だけに絞って返す（公開 project は
    // 含めぬ）。frontend の project 解決（projectKey→UUID）がこの一覧に依存するため、
    // 403 ではなく絞って開く（apps/backend/docs/tenant-project-authz.md）
    if let Some(guest_project_ids) = auth
        .ensure_tenant_access_or_guest_scope(&state, tenant_id)
        .await?
    {
        let list = projects::Entity::find()
            .filter(projects::Column::TenantId.eq(tenant_id))
            .filter(projects::Column::IsPersonal.eq(false))
            .filter(projects::Column::Id.is_in(guest_project_ids.into_iter().collect::<Vec<_>>()))
            .all(&state.db)
            .await?;
        return Ok(Json(list.into_iter().map(Into::into).collect()));
    }
    if is_tenant_owner(&state.db, tenant_id, auth.user_id).await? {
        let list = projects::Entity::find()
            .filter(projects::Column::TenantId.eq(tenant_id))
            .filter(projects::Column::IsPersonal.eq(false))
            .all(&state.db)
            .await?;
        return Ok(Json(list.into_iter().map(Into::into).collect()));
    }

    // テナントメンバーには、メンバーを 1 人も指定していないプロジェクトと
    // 自分が指定されたプロジェクトを返す（#568）
    let candidates = projects::Entity::find()
        .filter(projects::Column::TenantId.eq(tenant_id))
        .filter(projects::Column::IsPersonal.eq(false))
        .all(&state.db)
        .await?;

    let visible_ids = visible_project_ids(
        &state.db,
        candidates.iter().map(|p| p.id).collect(),
        auth.user_id,
    )
    .await?;
    let visible: Vec<_> = candidates
        .into_iter()
        .filter(|p| visible_ids.contains(&p.id))
        .collect();
    Ok(Json(visible.into_iter().map(Into::into).collect()))
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "Projects",
    summary = "プロジェクトを取得",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("id" = Uuid, Path, description = "プロジェクトID"),
    ),
    responses(
        (status = 200, description = "プロジェクト情報", body = ProjectResponse),
        CrudErrors,
    )
)]
pub async fn get_project(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ProjectResponse>, AppError> {
    auth.require_scope(Scope::ReadProject)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(id))
        .await?;
    let project = projects::Entity::find_by_id(id)
        .filter(projects::Column::TenantId.eq(tenant_id))
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(project.into()))
}

#[axum::debug_handler]
#[utoipa::path(
    put,
    path = "/{id}",
    tag = "Projects",
    summary = "プロジェクトを更新",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("id" = Uuid, Path, description = "プロジェクトID"),
    ),
    request_body = UpdateProjectRequest,
    responses(
        (status = 200, description = "更新後のプロジェクト", body = ProjectResponse),
        CrudErrors,
    )
)]
pub async fn update_project(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, id)): Path<(Uuid, Uuid)>,
    Valid(Json(payload)): Valid<Json<UpdateProjectRequest>>,
) -> Result<Json<ProjectResponse>, AppError> {
    auth.require_scope(Scope::WriteProject)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(id))
        .await?;
    require_tenant_owner(&state, tenant_id, auth.user_id).await?;
    let project = projects::Entity::find_by_id(id)
        .filter(projects::Column::TenantId.eq(tenant_id))
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    let mut active: projects::ActiveModel = project.into();
    if let Some(name) = payload.name {
        active.name = Set(name);
    }
    if let Some(description) = payload.description {
        active.description = Set(description);
    }
    if payload.clear_icon_emoji {
        active.icon_emoji = Set(None);
    } else if let Some(icon_emoji) = payload.icon_emoji {
        active.icon_emoji = Set(Some(icon_emoji));
    }
    if payload.clear_icon_url {
        active.icon_url = Set(None);
    } else if let Some(icon_url) = payload.icon_url {
        active.icon_url = Set(Some(icon_url));
    }
    let updated = active.update(&state.db).await?;
    Ok(Json(updated.into()))
}

#[axum::debug_handler]
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "Projects",
    summary = "プロジェクトを削除",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("id" = Uuid, Path, description = "プロジェクトID"),
    ),
    responses(
        (status = 204, description = "削除しました"),
        CrudErrors,
    )
)]
pub async fn delete_project(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    auth.require_scope(Scope::WriteProject)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(id))
        .await?;
    require_tenant_owner(&state, tenant_id, auth.user_id).await?;
    projects::Entity::find_by_id(id)
        .filter(projects::Column::TenantId.eq(tenant_id))
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    projects::Entity::delete_by_id(id).exec(&state.db).await?;
    Ok(StatusCode::NO_CONTENT)
}
