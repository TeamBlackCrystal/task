//! 管理者専用 — ユーザー管理 API（`/v1/admin/users`）

use crate::AppState;
use crate::error::AppError;
use crate::extractors::AdminUser;
use crate::handlers::admin_audit::record_audit;
use crate::openapi::CrudErrors;
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use axum_valid::Valid;
use chrono::Utc;
use common::db::{column_exists, execute_bound, table_exists};
use entity::{personal_tokens, tasks, tenant_members, users};
use payload::admin_users::*;
use payload::users::UserResponse;
use sea_orm::prelude::Uuid;
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait,
    PaginatorTrait, QueryFilter, TransactionTrait,
};
use service::auth::AuthError;
use service::auth::{create_password_hash, generate_email_verification_token};
use service::db::is_postgres_unique_violation;
use service::email::normalize_email;
use service::password_reset;
use service::password_reset_delivery;

fn auth_error_to_app(e: AuthError) -> AppError {
    match e {
        AuthError::Internal(err) => AppError::Internal(err),
        _ => AppError::Internal(anyhow::anyhow!("{e}")),
    }
}

async fn revoke_user_sessions(db: &DatabaseConnection, user_id: Uuid) -> Result<(), AppError> {
    users::Entity::update_many()
        .col_expr(
            users::Column::SessionsRevokedAt,
            Expr::value(Some(Utc::now())),
        )
        .filter(users::Column::Id.eq(user_id))
        .exec(db)
        .await?;
    Ok(())
}

async fn revoke_personal_tokens_for_user(
    db: &DatabaseConnection,
    user_id: Uuid,
) -> Result<(), AppError> {
    personal_tokens::Entity::update_many()
        .col_expr(personal_tokens::Column::Revoked, Expr::value(true))
        .filter(personal_tokens::Column::UserId.eq(user_id))
        .filter(personal_tokens::Column::Revoked.eq(false))
        .exec(db)
        .await?;
    Ok(())
}

async fn reset_2fa_for_user(db: &DatabaseConnection, user_id: Uuid) -> Result<(), AppError> {
    if table_exists(db, "totp_credentials").await? {
        execute_bound(
            db,
            "DELETE FROM totp_credentials WHERE user_id = ?",
            vec![user_id.into()],
        )
        .await?;
    }
    if table_exists(db, "recovery_codes").await? {
        execute_bound(
            db,
            "DELETE FROM recovery_codes WHERE user_id = ?",
            vec![user_id.into()],
        )
        .await?;
    }
    if column_exists(db, "users", "totp_enabled").await? {
        execute_bound(
            db,
            "UPDATE users SET totp_enabled = false WHERE id = ?",
            vec![user_id.into()],
        )
        .await?;
    }
    revoke_user_sessions(db, user_id).await?;
    Ok(())
}

/// 最後の1人の管理者を削除/降格できないようにするガード。
pub async fn ensure_not_last_admin(
    db: &DatabaseConnection,
    target_id: Uuid,
) -> Result<(), AppError> {
    let target = users::Entity::find_by_id(target_id)
        .one(db)
        .await?
        .ok_or(AppError::NotFound)?;
    if !target.is_admin {
        return Ok(());
    }
    let admin_count = users::Entity::find()
        .filter(users::Column::IsAdmin.eq(true))
        .count(db)
        .await?;
    if admin_count <= 1 {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

async fn delete_user_cascade(db: &DatabaseConnection, user_id: Uuid) -> Result<(), AppError> {
    if table_exists(db, "tasks").await? && column_exists(db, "tasks", "deleted_at").await? {
        let owned = tasks::Entity::find()
            .filter(tasks::Column::CreatedBy.eq(user_id))
            .filter(tasks::Column::DeletedAt.is_null())
            .all(db)
            .await?;
        let now = Utc::now();
        for task in owned {
            let mut active: tasks::ActiveModel = task.into();
            active.deleted_at = Set(Some(now.into()));
            active.updated_at = Set(now.into());
            active.update(db).await?;
        }
    }

    if table_exists(db, "task_assignees").await? {
        execute_bound(
            db,
            "DELETE FROM task_assignees WHERE user_id = ?",
            vec![user_id.into()],
        )
        .await?;
    }

    // テナントから外す（`tenant_members::remove_member`）のと同じ形にする。
    // `project_members` の行は消さない。消すと、その人しか指定されていなかったプロジェクトが
    // メンバー 0 件になり、テナント全体に開放されてしまう（#568）。
    // 残った行は project-only の客分の名指しに当たるが、この削除は PAT を revoke し
    // users の行を墓標化するため、削除済み利用者がそこから入ることはない
    // （客分の定めは apps/backend/docs/tenant-project-authz.md の「所属の 3 層」）。
    // users の行は墓標として残す方式なので、FK の ON DELETE CASCADE は発火しない
    if table_exists(db, "tenant_members").await? {
        tenant_members::Entity::delete_many()
            .filter(tenant_members::Column::UserId.eq(user_id))
            .exec(db)
            .await?;
    }

    if table_exists(db, "personal_tokens").await?
        && column_exists(db, "personal_tokens", "revoked").await?
    {
        personal_tokens::Entity::update_many()
            .col_expr(personal_tokens::Column::Revoked, Expr::value(true))
            .filter(personal_tokens::Column::UserId.eq(user_id))
            .exec(db)
            .await?;
    }

    if column_exists(db, "users", "sessions_revoked_at").await? {
        revoke_user_sessions(db, user_id).await?;
    }

    if table_exists(db, "totp_credentials").await? || table_exists(db, "recovery_codes").await? {
        reset_2fa_for_user(db, user_id).await?;
    }

    if table_exists(db, "passkeys").await? {
        execute_bound(
            db,
            "DELETE FROM passkeys WHERE user_id = ?",
            vec![user_id.into()],
        )
        .await?;
    }
    if table_exists(db, "oauth_connections").await? {
        execute_bound(
            db,
            "DELETE FROM oauth_connections WHERE user_id = ?",
            vec![user_id.into()],
        )
        .await?;
    }

    let txn = db.begin().await?;

    let tombstone_email = format!("deleted+{user_id}@invalid.local");
    let tombstone_username = format!("deleted-{user_id}");
    let user = users::Entity::find_by_id(user_id)
        .one(&txn)
        .await?
        .ok_or(AppError::NotFound)?;
    let mut active: users::ActiveModel = user.into();
    active.email = Set(tombstone_email);
    active.username = Set(tombstone_username);
    active.password_hash = Set(None);
    active.is_suspended = Set(true);
    active.is_admin = Set(false);
    active.email_verified = Set(false);
    active.update(&txn).await?;
    txn.commit().await?;
    Ok(())
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/",
    tag = "Admin Users",
    summary = "全ユーザー一覧",
    responses(
        (status = 200, description = "ユーザー一覧", body = [UserResponse]),
        CrudErrors,
    )
)]
pub async fn list_users(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<Json<Vec<UserResponse>>, AppError> {
    let list = users::Entity::find().all(&state.db).await?;
    Ok(Json(list.into_iter().map(Into::into).collect()))
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/",
    tag = "Admin Users",
    summary = "ユーザー作成（管理者）",
    request_body = AdminCreateUserRequest,
    responses(
        (status = 201, description = "作成されたユーザー", body = UserResponse),
        CrudErrors,
    )
)]
pub async fn create_user(
    State(state): State<AppState>,
    admin: AdminUser,
    headers: HeaderMap,
    Valid(Json(payload)): Valid<Json<AdminCreateUserRequest>>,
) -> Result<(StatusCode, Json<UserResponse>), AppError> {
    let email = normalize_email(&payload.email);
    let password_hash = create_password_hash(&payload.password).map_err(auth_error_to_app)?;
    let user_id = Uuid::new_v4();

    let user = users::ActiveModel {
        id: Set(user_id),
        username: Set(payload.username),
        bio: Set(Some(String::new())),
        avatar_url: Set(None),
        email: Set(email),
        email_verified: Set(payload.email_verified),
        password_hash: Set(Some(password_hash)),
        is_admin: Set(payload.is_admin),
        is_suspended: Set(false),
        sessions_revoked_at: Set(None),
        totp_enabled: Set(false),
    };

    let model = user.insert(&state.db).await.map_err(|e| {
        if is_postgres_unique_violation(&e) {
            AppError::Conflict
        } else {
            AppError::from(e)
        }
    })?;

    record_audit(
        &state.db,
        admin.user_id,
        "user.create",
        "user",
        &user_id.to_string(),
        None,
        Some(serde_json::json!({
            "email": model.email,
            "is_admin": model.is_admin,
        })),
        &headers,
    )
    .await?;

    Ok((StatusCode::CREATED, Json(model.into())))
}

#[axum::debug_handler]
#[utoipa::path(
    patch,
    path = "/{id}",
    tag = "Admin Users",
    summary = "ユーザー更新（is_admin / is_suspended）",
    request_body = AdminUpdateUserRequest,
    responses(
        (status = 200, description = "更新後のユーザー", body = UserResponse),
        CrudErrors,
    )
)]
pub async fn update_user(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Valid(Json(payload)): Valid<Json<AdminUpdateUserRequest>>,
) -> Result<Json<UserResponse>, AppError> {
    if payload.is_admin == Some(false) && admin.user_id == id {
        return Err(AppError::Forbidden);
    }

    let user = users::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    if payload.is_admin == Some(false) && user.is_admin {
        ensure_not_last_admin(&state.db, id).await?;
    }

    if payload.is_suspended == Some(true) && user.is_admin {
        ensure_not_last_admin(&state.db, id).await?;
    }

    let before_admin = user.is_admin;
    let before_suspended = user.is_suspended;

    if !before_suspended && payload.is_suspended == Some(true) {
        revoke_user_sessions(&state.db, id).await?;
        revoke_personal_tokens_for_user(&state.db, id).await?;
    }

    let mut active: users::ActiveModel = user.into();
    if let Some(v) = payload.is_admin {
        active.is_admin = Set(v);
    }
    if let Some(v) = payload.is_suspended {
        active.is_suspended = Set(v);
    }
    let model = active.update(&state.db).await?;

    if let Some(is_admin) = payload.is_admin
        && is_admin != before_admin
    {
        let action = if is_admin {
            "user.admin.grant"
        } else {
            "user.admin.revoke"
        };
        record_audit(
            &state.db,
            admin.user_id,
            action,
            "user",
            &id.to_string(),
            None,
            Some(serde_json::json!({ "before": before_admin, "after": is_admin })),
            &headers,
        )
        .await?;
    }

    if let Some(is_suspended) = payload.is_suspended
        && is_suspended != before_suspended
    {
        let action = if is_suspended {
            "user.suspend"
        } else {
            "user.unsuspend"
        };
        record_audit(
            &state.db,
            admin.user_id,
            action,
            "user",
            &id.to_string(),
            None,
            Some(serde_json::json!({ "before": before_suspended, "after": is_suspended })),
            &headers,
        )
        .await?;
    }

    Ok(Json(model.into()))
}

#[axum::debug_handler]
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "Admin Users",
    summary = "ユーザー強制削除",
    responses(
        (status = 204, description = "削除完了"),
        CrudErrors,
    )
)]
pub async fn delete_user(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    if admin.user_id == id {
        return Err(AppError::Forbidden);
    }

    let user = users::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    if user.is_admin {
        ensure_not_last_admin(&state.db, id).await?;
    }

    delete_user_cascade(&state.db, id).await?;

    record_audit(
        &state.db,
        admin.user_id,
        "user.delete",
        "user",
        &id.to_string(),
        None,
        Some(serde_json::json!({ "email": user.email })),
        &headers,
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/{id}/password-reset",
    tag = "Admin Users",
    summary = "パスワードリセットリンク生成・送信",
    request_body = AdminPasswordResetRequest,
    responses(
        (status = 200, description = "リセットメール送信", body = AdminPasswordResetResponse),
        CrudErrors,
    )
)]
pub async fn password_reset(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Valid(Json(payload)): Valid<Json<AdminPasswordResetRequest>>,
) -> Result<Json<AdminPasswordResetResponse>, AppError> {
    let user = users::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    let send_to = payload
        .send_to
        .as_ref()
        .map(|e| normalize_email(e))
        .unwrap_or_else(|| user.email.clone());

    if !password_reset::try_acquire_rate_limit(&state.redis_client, &send_to)
        .await
        .map_err(AppError::Internal)?
    {
        return Err(AppError::Conflict);
    }

    let token = generate_email_verification_token();
    password_reset::store_token(&state.redis_client, id, &token)
        .await
        .map_err(AppError::Internal)?;

    password_reset_delivery::send_password_reset_email(
        &state.smtp_client,
        &send_to,
        &state.settings,
        &token,
    )
    .await
    .map_err(AppError::Internal)?;

    record_audit(
        &state.db,
        admin.user_id,
        "user.password_reset",
        "user",
        &id.to_string(),
        None,
        Some(serde_json::json!({ "reset_email": send_to })),
        &headers,
    )
    .await?;

    Ok(Json(AdminPasswordResetResponse {
        message: "パスワードリセットメールを送信しました".into(),
    }))
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/{id}/reset-2fa",
    tag = "Admin Users",
    summary = "2FA 強制リセット",
    responses(
        (status = 204, description = "2FA リセット完了"),
        CrudErrors,
    )
)]
pub async fn reset_2fa(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    if users::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .is_none()
    {
        return Err(AppError::NotFound);
    }

    reset_2fa_for_user(&state.db, id).await?;

    record_audit(
        &state.db,
        admin.user_id,
        "user.2fa.reset",
        "user",
        &id.to_string(),
        None,
        None,
        &headers,
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

#[axum::debug_handler]
#[utoipa::path(
    delete,
    path = "/{id}/passkeys/{passkey_id}",
    // passkeys::delete_passkey と関数名が同じなため operationId を明示して重複を避ける
    operation_id = "delete_admin_user_passkey",
    tag = "Admin Users",
    summary = "パスキー強制削除",
    responses(
        (status = 204, description = "削除完了"),
        CrudErrors,
    )
)]
pub async fn delete_passkey(
    State(state): State<AppState>,
    admin: AdminUser,
    Path((id, passkey_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    if users::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .is_none()
    {
        return Err(AppError::NotFound);
    }

    if !table_exists(&state.db, "passkeys").await? {
        return Err(AppError::NotFound);
    }

    let result = execute_bound(
        &state.db,
        "DELETE FROM passkeys WHERE id = ? AND user_id = ?",
        vec![passkey_id.into(), id.into()],
    )
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    record_audit(
        &state.db,
        admin.user_id,
        "user.passkey.delete",
        "user",
        &id.to_string(),
        None,
        Some(serde_json::json!({ "passkey_id": passkey_id })),
        &headers,
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

#[axum::debug_handler]
#[utoipa::path(
    delete,
    path = "/{id}/oauth/{provider}",
    tag = "Admin Users",
    summary = "OAuth 連携強制解除",
    responses(
        (status = 204, description = "解除完了"),
        CrudErrors,
    )
)]
pub async fn delete_oauth(
    State(state): State<AppState>,
    admin: AdminUser,
    Path((id, provider)): Path<(Uuid, String)>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    if users::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .is_none()
    {
        return Err(AppError::NotFound);
    }

    if !table_exists(&state.db, "oauth_connections").await? {
        return Err(AppError::NotFound);
    }

    let result = execute_bound(
        &state.db,
        "DELETE FROM oauth_connections WHERE user_id = ? AND provider = ?",
        vec![id.into(), provider.clone().into()],
    )
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    record_audit(
        &state.db,
        admin.user_id,
        "user.oauth.disconnect",
        "user",
        &id.to_string(),
        None,
        Some(serde_json::json!({ "provider": provider })),
        &headers,
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}
