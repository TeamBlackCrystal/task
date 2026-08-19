use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use axum_session::Session;
use axum_session_redispool::SessionRedisPool;
use axum_valid::Valid;
use sea_orm::prelude::Uuid;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait};
use sea_orm::{ColumnTrait, QueryFilter};

use crate::AppState;
use crate::extractors::{AuthUser, CurrentUser};
use crate::openapi::{
    CredentialErrors, RegisterErrors, ResendVerificationErrors, SessionAuthErrors,
    UnauthorizedErrors, VerifyEmailErrors,
};
use entity::{system_settings, users};
use job::AlreadyRegisteredEmailJob;
use job::VerificationEmailJob;
use job::already_registered_email;
use job::verification_email;
use payload::auth::*;
use payload::auth_2fa::Login2faResponse;
use payload::users::{UpdateProfileRequest, UserResponse};
use service::auth::{AuthError, create_password_hash, dummy_password_hash, verify_password};
use service::db::{is_postgres_unique_violation, with_transaction};
use service::email::normalize_email;
use service::email_verification;
use service::login_session::establish_login_session;

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/login",
    tag = "Auth",
    summary = "ログイン",
    request_body = LoginRequest,
    responses(
        (status = 204, description = "ログインに成功しました（本文なし）"),
        (status = 200, description = "2FA が必要", body = Login2faResponse),
        CredentialErrors,
    )
)]
pub async fn login(
    session: Session<SessionRedisPool>,
    State(state): State<AppState>,
    Valid(Json(payload)): Valid<Json<LoginRequest>>,
) -> Result<impl IntoResponse, AuthError> {
    let LoginRequest { email, password } = payload;
    let email = normalize_email(&email);

    let user = users::Entity::find()
        .filter(users::Column::Email.eq(&email))
        .one(&state.db)
        .await?;

    let password_hash = match user.as_ref().and_then(|u| u.password_hash.as_deref()) {
        Some(hash) => hash,
        None => dummy_password_hash()?,
    };

    if !verify_password(&password, password_hash)? {
        return Err(AuthError::InvalidCredentials);
    }

    let user = user.ok_or(AuthError::InvalidCredentials)?;

    if !user.email_verified {
        return Err(AuthError::EmailNotVerified);
    }

    if let Some(response) = establish_login_session(&session, &state.db, &user).await? {
        return Ok((StatusCode::OK, Json(response)).into_response());
    }

    Ok(StatusCode::NO_CONTENT.into_response())
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/register",
    tag = "Auth",
    summary = "新規登録",
    request_body = RegisterRequest,
    responses(
        (
            status = 201,
            description = "リクエストを受け付けました。続けて送信されたメールをご確認ください（メールアドレスが既に登録済みの場合も、列挙対策のため同一レスポンスを返します）。",
            body = String
        ),
        RegisterErrors,
    )
)]
pub async fn register(
    State(state): State<AppState>,
    Valid(Json(payload)): Valid<Json<RegisterRequest>>,
) -> Result<(StatusCode, Json<String>), AuthError> {
    let RegisterRequest {
        username,
        email,
        password,
    } = payload;
    let email = normalize_email(&email);

    service::system_settings::ensure_system_settings_row(&state.db).await?;

    let settings = system_settings::Entity::find()
        .filter(system_settings::Column::Singleton.eq(true))
        .one(&state.db)
        .await?
        .ok_or(AuthError::Internal(anyhow::anyhow!(
            "system_settings singleton row missing after ensure"
        )))?;

    if !settings.user_registration_enabled {
        return Err(AuthError::Forbidden);
    }

    // #26: 既知アドレスへのメールフラッディング対策。存在オラクルにならないよう、
    // 新規/既存の分岐より前で一律にレート制限を掛ける（枠なしは常に 429）。
    if !email_verification::try_acquire_register_slot(&state.redis_client, &email)
        .await
        .map_err(|e| AuthError::Internal(anyhow::anyhow!("redis register cooldown: {e}")))?
    {
        return Err(AuthError::TooManyRequests);
    }

    let password_hash = create_password_hash(&password)?;

    let user = users::ActiveModel {
        id: Set(Uuid::new_v4()),
        username: Set(username),
        bio: Set(Some(String::new())),
        avatar_url: Set(None),
        email: Set(email.clone()),
        email_verified: Set(false),
        password_hash: Set(Some(password_hash)),
        is_admin: Set(false),
        is_suspended: Set(false),
        sessions_revoked_at: Set(None),
        totp_enabled: Set(false),
    };

    let insert_result = with_transaction::<Uuid, AuthError, _>(&state.db, |txn| {
        Box::pin(async move {
            let res = users::Entity::insert(user.clone())
                .exec(txn)
                .await
                .map_err(|e| {
                    if is_postgres_unique_violation(&e) {
                        AuthError::DuplicateEmail
                    } else {
                        AuthError::Internal(anyhow::anyhow!("insert user: {e}"))
                    }
                })?;

            Ok(res.last_insert_id)
        })
    })
    .await;

    // #26: メールアドレス列挙対策。既存メールでも未使用時と同一の 201 レスポンスを返す。
    // 既存メール宛には確認メールの代わりに「登録済みです」通知メールを送る。
    // 送信キュー投入失敗時も分岐によらず同じ AuthError（同一レスポンス）にする。
    match insert_result {
        Ok(user_id) => {
            verification_email::enqueue(
                state.verification_email_storage.as_ref(),
                VerificationEmailJob::new(user_id, email.clone()),
            )
            .await
            .map_err(AuthError::VerificationEmailEnqueueFailed)?;
        }
        Err(AuthError::DuplicateEmail) => {
            already_registered_email::enqueue(
                state.already_registered_email_storage.as_ref(),
                AlreadyRegisteredEmailJob::new(email.clone()),
            )
            .await
            .map_err(AuthError::VerificationEmailEnqueueFailed)?;
        }
        Err(e) => return Err(e),
    }

    Ok((StatusCode::CREATED, Json("Register successful".to_string())))
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/verify-email",
    tag = "Auth",
    summary = "メールアドレスの確認",
    request_body = VerifyEmailRequest,
    responses(
        (
            status = 200,
            description = "メールアドレスの確認が完了しました",
            body = String
        ),
        VerifyEmailErrors,
    )
)]
pub async fn verify_email(
    State(state): State<AppState>,
    Valid(Json(payload)): Valid<Json<VerifyEmailRequest>>,
) -> Result<Json<String>, AuthError> {
    let user_id = email_verification::consume_token(&state.redis_client, &payload.token)
        .await
        .map_err(|e| AuthError::Internal(anyhow::anyhow!("redis consume verification token: {e}")))?
        .ok_or(AuthError::InvalidVerificationToken)?;

    let user = users::Entity::find_by_id(user_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| {
            AuthError::Internal(anyhow::anyhow!(
                "email verification token referenced missing user"
            ))
        })?;

    if user.email_verified {
        return Ok(Json("Email already verified".to_string()));
    }

    let mut active: users::ActiveModel = user.into();
    active.email_verified = Set(true);
    active.update(&state.db).await?;

    Ok(Json("Email verified".to_string()))
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/resend-verification-email",
    tag = "Auth",
    summary = "認証メールの再送",
    request_body = ResendVerificationRequest,
    responses(
        (status = 200, description = "認証メールを送信しました", body = String),
        ResendVerificationErrors,
    )
)]
pub async fn resend_verification_email(
    State(state): State<AppState>,
    Valid(Json(payload)): Valid<Json<ResendVerificationRequest>>,
) -> Result<Json<String>, AuthError> {
    let email = normalize_email(&payload.email);

    if !email_verification::try_acquire_resend_slot(&state.redis_client, &email)
        .await
        .map_err(|e| AuthError::Internal(anyhow::anyhow!("redis resend cooldown: {e}")))?
    {
        return Err(AuthError::TooManyRequests);
    }

    let user = users::Entity::find()
        .filter(users::Column::Email.eq(email.clone()))
        .one(&state.db)
        .await?
        .ok_or(AuthError::UserNotFound)?;

    if user.email_verified {
        return Err(AuthError::EmailAlreadyVerified);
    }

    verification_email::enqueue(
        state.verification_email_storage.as_ref(),
        VerificationEmailJob::new(user.id, email.clone()),
    )
    .await
    .map_err(AuthError::VerificationEmailEnqueueFailed)?;

    Ok(Json(format!(
        "確認メールを再送しました（同一メールアドレスへの再送は{}秒に1回までです）。",
        email_verification::RESEND_COOLDOWN_SECS
    )))
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/me",
    tag = "Auth",
    summary = "ログイン中ユーザー情報",
    responses(
        (status = 200, description = "現在のアカウント情報", body = UserResponse),
        SessionAuthErrors,
    )
)]
pub async fn me(
    State(_): State<AppState>,
    user: CurrentUser,
) -> Result<Json<UserResponse>, AuthError> {
    Ok(Json(user.0.into()))
}

#[axum::debug_handler]
#[utoipa::path(
    patch,
    path = "/me",
    tag = "Auth",
    summary = "自分のプロフィール更新",
    request_body = UpdateProfileRequest,
    responses(
        (status = 200, description = "更新後のアカウント情報", body = UserResponse),
        (status = 400, description = "入力内容が制約を満たしていません"),
        SessionAuthErrors,
    )
)]
pub async fn update_me(
    State(state): State<AppState>,
    user: CurrentUser,
    Valid(Json(payload)): Valid<Json<UpdateProfileRequest>>,
) -> Result<Json<UserResponse>, AuthError> {
    let UpdateProfileRequest {
        username,
        bio,
        avatar_url,
        clear_avatar_url,
    } = payload;

    let current = user.0;
    let mut active: users::ActiveModel = current.clone().into();

    if let Some(username) = username {
        active.username = Set(username);
    }
    if let Some(bio) = bio {
        active.bio = Set(Some(bio));
    }
    if clear_avatar_url {
        active.avatar_url = Set(None);
    } else if let Some(avatar_url) = avatar_url {
        active.avatar_url = Set(Some(avatar_url));
    }

    // 変更なしの PATCH で SeaORM に空の UPDATE を投げない。
    if !active.is_changed() {
        return Ok(Json(current.into()));
    }

    Ok(Json(active.update(&state.db).await?.into()))
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/logout",
    tag = "Auth",
    summary = "ログアウト",
    responses(
        (status = 204, description = "ログアウトしました（本文なし）"),
        UnauthorizedErrors,
    )
)]
pub async fn logout(
    session: Session<SessionRedisPool>,
    State(_): State<AppState>,
    _auth: AuthUser,
) -> Result<StatusCode, AuthError> {
    session.remove("user_id");
    session.remove("half_authed");
    Ok(StatusCode::NO_CONTENT)
}
