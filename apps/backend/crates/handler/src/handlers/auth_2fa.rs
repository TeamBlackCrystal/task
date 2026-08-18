use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use axum_session::Session;
use axum_session_redispool::SessionRedisPool;
use axum_valid::Valid;
use chrono::Utc;
use sea_orm::prelude::Uuid;
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, JoinType, QueryFilter,
    QuerySelect, RelationTrait, TransactionTrait,
};

use entity::{recovery_codes, tenant_members, tenants, totp_credentials, users};

use crate::{
    AppState,
    error::AppError,
    error::ServerError,
    extractors::{AuthUser, HalfAuthedUser, LoggedInUser},
    openapi::{CrudErrors, SessionAuthErrors},
};
use payload::auth_2fa::*;
use payload::tenants::TenantResponse;
use service::auth::AuthError;
use service::totp::{
    self, clear_2fa_attempts, decrypt_totp_secret, encrypt_totp_secret, generate_recovery_codes,
    generate_totp_secret_base32, hash_recovery_code, normalize_recovery_code, otpauth_uri,
    qr_code_png_data_uri, recovery_code_matches, verify_totp_code,
};

pub async fn user_has_active_2fa(
    db: &sea_orm::DatabaseConnection,
    user_id: Uuid,
) -> Result<bool, AuthError> {
    let user = users::Entity::find_by_id(user_id)
        .one(db)
        .await?
        .ok_or(AuthError::Unauthorized)?;
    if !user.totp_enabled {
        return Ok(false);
    }
    let cred = totp_credentials::Entity::find_by_id(user_id)
        .one(db)
        .await?;
    Ok(cred.map(|c| c.is_verified).unwrap_or(false))
}

/// ユーザーが所属する（テナントオーナー or テナントメンバー）テナントのいずれかで
/// `require_2fa=true` が設定されているかを判定する。
/// 2FA セットアップ強制（`user_must_setup_2fa`）と 2FA 無効化禁止（`delete_totp`）の
/// 双方で参照する共通ポリシー判定。
async fn user_in_require_2fa_tenant(
    db: &sea_orm::DatabaseConnection,
    user_id: Uuid,
) -> Result<bool, AuthError> {
    let owns_required_tenant = tenants::Entity::find()
        .filter(tenants::Column::OwnerId.eq(user_id))
        .filter(tenants::Column::Require2fa.eq(true))
        .one(db)
        .await?
        .is_some();
    if owns_required_tenant {
        return Ok(true);
    }
    // 2FA 必須は「そのテナントに所属しているか」で決まるので、テナントメンバーを見る（#568）
    let member_of_required_tenant = tenant_members::Entity::find()
        .join(JoinType::InnerJoin, tenant_members::Relation::Tenants.def())
        .filter(tenant_members::Column::UserId.eq(user_id))
        .filter(tenants::Column::Require2fa.eq(true))
        .one(db)
        .await?
        .is_some();
    Ok(member_of_required_tenant)
}

async fn user_must_setup_2fa(
    db: &sea_orm::DatabaseConnection,
    user_id: Uuid,
) -> Result<bool, AuthError> {
    if user_has_active_2fa(db, user_id).await? {
        return Ok(false);
    }
    user_in_require_2fa_tenant(db, user_id).await
}

pub async fn login_2fa_flags(
    db: &sea_orm::DatabaseConnection,
    user: &users::Model,
) -> Result<(bool, bool), AuthError> {
    let requires_2fa = user_has_active_2fa(db, user.id).await?;
    let requires_2fa_setup = user_must_setup_2fa(db, user.id).await?;
    Ok((requires_2fa, requires_2fa_setup))
}

/// 第一認証（パスワード / OAuth コールバック）成功後のセッション確立。
/// 2FA 必須時は `half_authed` セッションを返す（OAuth 経路からも呼ぶ）。
pub async fn establish_login_session(
    session: &Session<SessionRedisPool>,
    db: &sea_orm::DatabaseConnection,
    user: &users::Model,
) -> Result<Option<Login2faResponse>, AuthError> {
    let (requires_2fa, requires_2fa_setup) = login_2fa_flags(db, user).await?;
    session.renew();
    session.set("issued_at_ms", Utc::now().timestamp_millis());
    session.set("user_id", user.id);
    if requires_2fa || requires_2fa_setup {
        session.set("half_authed", true);
        return Ok(Some(Login2faResponse {
            requires_2fa,
            requires_2fa_setup,
        }));
    }
    session.set("half_authed", false);
    Ok(None)
}

async fn load_user(state: &AppState, user_id: Uuid) -> Result<users::Model, AuthError> {
    users::Entity::find_by_id(user_id)
        .one(&state.db)
        .await?
        .ok_or(AuthError::Unauthorized)
}

async fn insert_recovery_code_hashes(
    txn: &sea_orm::DatabaseTransaction,
    user_id: Uuid,
    codes: &[String],
    recovery_secret: &str,
) -> Result<(), AuthError> {
    let created_at = Utc::now().into();
    let models: Result<Vec<recovery_codes::ActiveModel>, AuthError> = codes
        .iter()
        .map(|code| {
            let normalized = normalize_recovery_code(code);
            let hash = hash_recovery_code(&normalized, recovery_secret)?;
            Ok(recovery_codes::ActiveModel {
                id: Set(Uuid::new_v4()),
                user_id: Set(user_id),
                code_hash: Set(hash),
                used_at: Set(None),
                created_at: Set(created_at),
            })
        })
        .collect();
    recovery_codes::Entity::insert_many(models?)
        .exec(txn)
        .await?;
    Ok(())
}

async fn verify_user_totp_or_recovery(
    state: &AppState,
    user_id: Uuid,
    code: Option<&str>,
    recovery_code: Option<&str>,
) -> Result<(), AuthError> {
    totp::check_2fa_lockout(&state.redis_client, user_id).await?;

    if let Some(recovery) = recovery_code {
        let normalized = normalize_recovery_code(recovery);
        let candidate_hash = hash_recovery_code(&normalized, &state.settings.recovery_code_secret)?;

        let txn = state.db.begin().await?;
        let codes = recovery_codes::Entity::find()
            .filter(recovery_codes::Column::UserId.eq(user_id))
            .filter(recovery_codes::Column::UsedAt.is_null())
            .all(&txn)
            .await?;

        let mut matched_id: Option<Uuid> = None;
        for stored in codes {
            if recovery_code_matches(&stored.code_hash, &candidate_hash) {
                matched_id = Some(stored.id);
                break;
            }
        }

        if let Some(id) = matched_id {
            let result = recovery_codes::Entity::update_many()
                .col_expr(
                    recovery_codes::Column::UsedAt,
                    Expr::value(Some(Utc::now())),
                )
                .filter(recovery_codes::Column::Id.eq(id))
                .filter(recovery_codes::Column::UsedAt.is_null())
                .exec(&txn)
                .await?;

            if result.rows_affected == 1 {
                txn.commit().await?;
                clear_2fa_attempts(&state.redis_client, user_id).await?;
                return Ok(());
            }
            txn.rollback().await?;
            return Err(AuthError::InvalidTwoFactorCode);
        }

        txn.rollback().await?;
        totp::record_2fa_failure(&state.redis_client, user_id).await?;
        return Err(AuthError::InvalidTwoFactorCode);
    }

    let code = code.ok_or(AuthError::InvalidTwoFactorCode)?;
    let cred = totp_credentials::Entity::find_by_id(user_id)
        .one(&state.db)
        .await?
        .ok_or(AuthError::TwoFactorNotEnabled)?;
    if !cred.is_verified {
        return Err(AuthError::TwoFactorNotEnabled);
    }
    let secret = decrypt_totp_secret(&cred.secret_enc, &state.settings.totp_encryption_key)?;
    let user = users::Entity::find_by_id(user_id)
        .one(&state.db)
        .await?
        .ok_or(AuthError::Unauthorized)?;
    if verify_totp_code(
        &secret,
        &state.settings.totp_issuer,
        &user.email,
        code.trim(),
    )? {
        clear_2fa_attempts(&state.redis_client, user_id).await?;
        Ok(())
    } else {
        totp::record_2fa_failure(&state.redis_client, user_id).await?;
        Err(AuthError::InvalidTwoFactorCode)
    }
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/2fa/totp/setup",
    tag = "Auth",
    summary = "TOTP 2FA セットアップ開始",
    responses(
        (status = 200, description = "QR コード用データ", body = TotpSetupResponse),
        SessionAuthErrors,
    )
)]
pub async fn totp_setup(
    State(state): State<AppState>,
    user: LoggedInUser,
) -> Result<Json<TotpSetupResponse>, AuthError> {
    let user_model = load_user(&state, user.user_id).await?;
    if user_model.totp_enabled {
        return Err(AuthError::TwoFactorAlreadyEnabled);
    }

    let secret_plain = generate_totp_secret_base32()?;
    let secret_enc = encrypt_totp_secret(&secret_plain, &state.settings.totp_encryption_key)?;
    let now = Utc::now();

    totp_credentials::Entity::delete_many()
        .filter(totp_credentials::Column::UserId.eq(user.user_id))
        .exec(&state.db)
        .await?;

    totp_credentials::ActiveModel {
        user_id: Set(user.user_id),
        secret_enc: Set(secret_enc),
        is_verified: Set(false),
        created_at: Set(now.into()),
    }
    .insert(&state.db)
    .await?;

    let uri = otpauth_uri(
        &secret_plain,
        &state.settings.totp_issuer,
        &user_model.email,
    )?;
    let qr = qr_code_png_data_uri(&uri)?;

    Ok(Json(TotpSetupResponse {
        otpauth_uri: uri,
        qr_code_png: qr,
    }))
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/2fa/totp/verify-setup",
    tag = "Auth",
    summary = "TOTP 初回コード検証・有効化",
    request_body = TotpCodeRequest,
    responses(
        (status = 200, description = "リカバリーコード（初回のみ）", body = VerifySetupResponse),
        SessionAuthErrors,
    )
)]
pub async fn totp_verify_setup(
    session: Session<SessionRedisPool>,
    State(state): State<AppState>,
    user: LoggedInUser,
    Valid(Json(payload)): Valid<Json<TotpCodeRequest>>,
) -> Result<Json<VerifySetupResponse>, AuthError> {
    let user_model = load_user(&state, user.user_id).await?;
    if user_model.totp_enabled {
        return Err(AuthError::TwoFactorAlreadyEnabled);
    }

    let cred = totp_credentials::Entity::find_by_id(user.user_id)
        .one(&state.db)
        .await?
        .ok_or(AuthError::TwoFactorNotEnabled)?;
    if cred.is_verified {
        return Err(AuthError::TwoFactorAlreadyEnabled);
    }

    totp::check_2fa_lockout(&state.redis_client, user.user_id).await?;

    let secret = decrypt_totp_secret(&cred.secret_enc, &state.settings.totp_encryption_key)?;
    if !verify_totp_code(
        &secret,
        &state.settings.totp_issuer,
        &user_model.email,
        payload.code.trim(),
    )? {
        totp::record_2fa_failure(&state.redis_client, user.user_id).await?;
        return Err(AuthError::InvalidTwoFactorCode);
    }

    let codes = generate_recovery_codes();
    let txn = state.db.begin().await?;

    let mut cred_active: totp_credentials::ActiveModel = cred.into();
    cred_active.is_verified = Set(true);
    cred_active.update(&txn).await?;

    recovery_codes::Entity::delete_many()
        .filter(recovery_codes::Column::UserId.eq(user.user_id))
        .exec(&txn)
        .await?;

    insert_recovery_code_hashes(
        &txn,
        user.user_id,
        &codes,
        &state.settings.recovery_code_secret,
    )
    .await?;

    let mut user_active: users::ActiveModel = user_model.into();
    user_active.totp_enabled = Set(true);
    user_active.update(&txn).await?;

    txn.commit().await?;

    clear_2fa_attempts(&state.redis_client, user.user_id).await?;
    session.set("half_authed", false);
    session.renew();

    Ok(Json(VerifySetupResponse {
        recovery_codes: codes,
    }))
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/2fa/verify",
    tag = "Auth",
    summary = "ログイン後の TOTP / リカバリーコード検証",
    request_body = Verify2faRequest,
    responses(
        (status = 204, description = "完全認証に昇格"),
        SessionAuthErrors,
    )
)]
pub async fn verify_2fa(
    session: Session<SessionRedisPool>,
    State(state): State<AppState>,
    user: HalfAuthedUser,
    Valid(Json(payload)): Valid<Json<Verify2faRequest>>,
) -> Result<StatusCode, AuthError> {
    if payload.code.is_none() && payload.recovery_code.is_none() {
        return Err(AuthError::InvalidTwoFactorCode);
    }
    verify_user_totp_or_recovery(
        &state,
        user.user_id,
        payload.code.as_deref(),
        payload.recovery_code.as_deref(),
    )
    .await?;
    session.set("half_authed", false);
    session.renew();
    Ok(StatusCode::NO_CONTENT)
}

#[axum::debug_handler]
#[utoipa::path(
    delete,
    path = "/2fa/totp",
    tag = "Auth",
    summary = "TOTP 2FA 無効化",
    request_body = Verify2faRequest,
    responses(
        (status = 204, description = "無効化完了"),
        SessionAuthErrors,
    )
)]
pub async fn delete_totp(
    State(state): State<AppState>,
    auth: AuthUser,
    Valid(Json(payload)): Valid<Json<Verify2faRequest>>,
) -> Result<StatusCode, AuthError> {
    auth.require_session().map_err(|_| AuthError::Forbidden)?;
    let user = load_user(&state, auth.user_id).await?;
    if !user.totp_enabled {
        return Err(AuthError::TwoFactorNotEnabled);
    }
    // テナントの 2FA 強制ポリシー（require_2fa=true）が有効なユーザーは
    // 自分で 2FA を無効化できない。コード検証を消費する前に弾く。
    if user_in_require_2fa_tenant(&state.db, auth.user_id).await? {
        return Err(AuthError::Forbidden);
    }
    verify_user_totp_or_recovery(
        &state,
        auth.user_id,
        payload.code.as_deref(),
        payload.recovery_code.as_deref(),
    )
    .await?;

    let txn = state.db.begin().await?;
    let mut user_active: users::ActiveModel = user.into();
    user_active.totp_enabled = Set(false);
    user_active.update(&txn).await?;
    totp_credentials::Entity::delete_by_id(auth.user_id)
        .exec(&txn)
        .await?;
    recovery_codes::Entity::delete_many()
        .filter(recovery_codes::Column::UserId.eq(auth.user_id))
        .exec(&txn)
        .await?;
    txn.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/2fa/recovery-codes/regenerate",
    tag = "Auth",
    summary = "リカバリーコード再生成",
    request_body = TotpCodeRequest,
    responses(
        (status = 200, description = "新しいリカバリーコード", body = VerifySetupResponse),
        SessionAuthErrors,
    )
)]
pub async fn regenerate_recovery_codes(
    State(state): State<AppState>,
    auth: AuthUser,
    Valid(Json(payload)): Valid<Json<TotpCodeRequest>>,
) -> Result<Json<VerifySetupResponse>, AuthError> {
    auth.require_session().map_err(|_| AuthError::Forbidden)?;
    let user = load_user(&state, auth.user_id).await?;
    if !user.totp_enabled {
        return Err(AuthError::TwoFactorNotEnabled);
    }
    verify_user_totp_or_recovery(&state, auth.user_id, Some(&payload.code), None).await?;

    let codes = generate_recovery_codes();
    let txn = state.db.begin().await?;
    recovery_codes::Entity::delete_many()
        .filter(recovery_codes::Column::UserId.eq(auth.user_id))
        .exec(&txn)
        .await?;
    insert_recovery_code_hashes(
        &txn,
        auth.user_id,
        &codes,
        &state.settings.recovery_code_secret,
    )
    .await?;
    txn.commit().await?;
    Ok(Json(VerifySetupResponse {
        recovery_codes: codes,
    }))
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/{tenant_id}/require-2fa",
    tag = "Tenants",
    summary = "テナント 2FA 強制ポリシー変更",
    request_body = Require2faPolicyRequest,
    responses(
        (status = 200, description = "更新後のテナント", body = TenantResponse),
        (status = 404, description = "テナントが見つかりません", body = ServerError),
        CrudErrors,
    )
)]
pub async fn set_tenant_require_2fa(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(tenant_id): Path<Uuid>,
    Valid(Json(payload)): Valid<Json<Require2faPolicyRequest>>,
) -> Result<Json<TenantResponse>, AppError> {
    auth.require_session()?;
    let tenant = auth.ensure_tenant_owner(&state, tenant_id).await?;

    let mut active: tenants::ActiveModel = tenant.into();
    active.require_2fa = Set(payload.enabled);
    let updated = active.update(&state.db).await?;
    Ok(Json(updated.into()))
}
