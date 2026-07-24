use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use axum_session::Session;
use axum_session_redispool::SessionRedisPool;
use axum_valid::Valid;
use chrono::Utc;
use sea_orm::prelude::Uuid;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter,
    QuerySelect,
};
use thiserror::Error;
use tracing::{debug, warn};

use crate::error::{ServerError, internal_server_error};
use crate::extractors::{AuthUser, CurrentUser, OptionalAuthUser};
use crate::openapi::OAuthErrors;
use entity::{oauth_connections, users};
use github_integration::oauth::client::{TokenResponse, exchange_code, fetch_user_info};
use github_integration::oauth::crypto::encrypt_token;
use github_integration::oauth::pkce::{generate_pkce_pair, generate_state};
use github_integration::oauth::provider::{
    ProviderUserInfo, build_authorize_url, get_credentials, normalize_instance_url,
    resolve_endpoints,
};
use github_integration::oauth::state::{
    OAuthStatePayload, build_frontend_oauth_error_redirect, build_frontend_redirect, consume_state,
    sanitize_redirect_path, store_state,
};
use service::auth::{AuthError, create_password_hash};
use service::db::{is_postgres_unique_violation, with_transaction};
use service::email::normalize_email;
use service::passkeys::count_user_passkeys;

use payload::oauth::*;
use service::login_session::establish_login_session;

use crate::AppState;

const OAUTH_PENDING_STATE_KEY: &str = "oauth_pending_state";
const OAUTH_PENDING_PROVIDER_KEY: &str = "oauth_pending_provider";

#[derive(Error, Debug)]
pub enum OAuthError {
    #[error("internal error")]
    Internal(#[from] anyhow::Error),
    #[error("provider not configured")]
    ProviderNotConfigured,
    #[error("invalid oauth state")]
    InvalidState,
    #[error("email conflict")]
    EmailConflict,
    #[error("username conflict")]
    UsernameConflict,
    #[error("connection already exists")]
    ConnectionExists,
    #[error("connection not found")]
    ConnectionNotFound,
    #[error("password already set")]
    PasswordAlreadySet,
    #[error("cannot remove last auth method")]
    LastAuthMethod,
    #[error("bad request")]
    BadRequest,
    #[error("unauthorized")]
    Unauthorized,
    #[error("account suspended")]
    AccountSuspended,
    #[error("security violation")]
    SecurityViolation,
}

impl From<sea_orm::DbErr> for OAuthError {
    fn from(err: sea_orm::DbErr) -> Self {
        OAuthError::Internal(err.into())
    }
}

impl From<AuthError> for OAuthError {
    fn from(err: AuthError) -> Self {
        match err {
            AuthError::Unauthorized => OAuthError::Unauthorized,
            other => OAuthError::Internal(anyhow::anyhow!("{other}")),
        }
    }
}

impl IntoResponse for OAuthError {
    fn into_response(self) -> Response {
        match self {
            OAuthError::Internal(e) => {
                debug!("oauth error: {:#?}", e);
                internal_server_error().into_response()
            }
            OAuthError::ProviderNotConfigured => (
                StatusCode::NOT_FOUND,
                Json(ServerError {
                    message: "oauth-provider-not-configured".into(),
                }),
            )
                .into_response(),
            OAuthError::InvalidState => (
                StatusCode::BAD_REQUEST,
                Json(ServerError {
                    message: "invalid-oauth-state".into(),
                }),
            )
                .into_response(),
            OAuthError::EmailConflict => (
                StatusCode::CONFLICT,
                Json(ServerError {
                    message: "oauth-email-conflict".into(),
                }),
            )
                .into_response(),
            OAuthError::UsernameConflict => (
                StatusCode::CONFLICT,
                Json(ServerError {
                    message: "oauth-username-conflict".into(),
                }),
            )
                .into_response(),
            OAuthError::ConnectionExists => (
                StatusCode::CONFLICT,
                Json(ServerError {
                    message: "oauth-connection-exists".into(),
                }),
            )
                .into_response(),
            OAuthError::ConnectionNotFound => (
                StatusCode::NOT_FOUND,
                Json(ServerError {
                    message: "oauth-connection-not-found".into(),
                }),
            )
                .into_response(),
            OAuthError::PasswordAlreadySet => (
                StatusCode::CONFLICT,
                Json(ServerError {
                    message: "password-already-set".into(),
                }),
            )
                .into_response(),
            OAuthError::LastAuthMethod => (
                StatusCode::FORBIDDEN,
                Json(ServerError {
                    message: "oauth-last-auth-method".into(),
                }),
            )
                .into_response(),
            OAuthError::BadRequest => (
                StatusCode::BAD_REQUEST,
                Json(ServerError {
                    message: "bad-request".into(),
                }),
            )
                .into_response(),
            OAuthError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                Json(ServerError {
                    message: "unauthorized".into(),
                }),
            )
                .into_response(),
            OAuthError::AccountSuspended => (
                StatusCode::FORBIDDEN,
                Json(ServerError {
                    message: "account-suspended".into(),
                }),
            )
                .into_response(),
            OAuthError::SecurityViolation => {
                warn!("oauth security validation failed");
                (
                    StatusCode::BAD_REQUEST,
                    Json(ServerError {
                        message: "bad-request".into(),
                    }),
                )
                    .into_response()
            }
        }
    }
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/oauth/{provider}",
    tag = "Auth",
    summary = "OAuth 認可 URL へリダイレクト",
    params(
        ("provider" = String, Path, description = "github | gitlab | gitlab_selfhosted | google | oidc"),
        ("redirect_after" = Option<String>, Query, description = "ログイン後のフロント相対パス"),
        ("error_redirect_after" = Option<String>, Query, description = "プロバイダーエラー時の戻り先（OAuth ボタンのあるページ）"),
        ("instance_url" = Option<String>, Query, description = "GitLab self-hosted インスタンス URL")
    ),
    responses(
        (status = 302, description = "プロバイダー認可 URL へリダイレクト"),
        OAuthErrors,
    )
)]
pub async fn oauth_start(
    Path(provider): Path<String>,
    Query(query): Query<OAuthStartQuery>,
    optional_auth: OptionalAuthUser,
    session: Session<SessionRedisPool>,
    State(state): State<AppState>,
) -> Result<Redirect, OAuthError> {
    let settings = &state.oauth_settings;

    if !settings.is_provider_configured(&provider) {
        return Err(OAuthError::ProviderNotConfigured);
    }

    let instance_url = match provider.as_str() {
        "gitlab_selfhosted" => Some(
            normalize_instance_url(
                query
                    .instance_url
                    .as_deref()
                    .ok_or(OAuthError::BadRequest)?,
            )
            .map_err(OAuthError::Internal)?,
        ),
        _ => None,
    };

    let endpoints = resolve_endpoints(
        &provider,
        settings,
        instance_url.as_deref(),
        &state.http_client,
    )
    .await
    .map_err(OAuthError::Internal)?;
    let credentials = get_credentials(&provider, settings).map_err(OAuthError::Internal)?;

    let pkce = generate_pkce_pair();
    let oauth_state = generate_state();
    let raw_redirect = query
        .redirect_after
        .as_deref()
        .unwrap_or(settings.default_redirect_path.as_str());
    let redirect_after = sanitize_redirect_path(raw_redirect).map_err(|e| {
        warn!("oauth redirect_after rejected: {e}");
        OAuthError::SecurityViolation
    })?;

    // プロバイダーがエラーを返したときの戻り先。未指定なら成功用にフォールバックする。
    let raw_error_redirect = query
        .error_redirect_after
        .as_deref()
        .unwrap_or(raw_redirect);
    let error_redirect_after = sanitize_redirect_path(raw_error_redirect).map_err(|e| {
        warn!("oauth error_redirect_after rejected: {e}");
        OAuthError::SecurityViolation
    })?;

    let link_user_id = optional_auth.0.map(|auth| auth.user_id);

    store_state(
        &state.redis_client,
        &oauth_state,
        &OAuthStatePayload {
            provider: provider.clone(),
            code_verifier: pkce.code_verifier,
            redirect_after,
            error_redirect_after,
            link_user_id,
            instance_url: instance_url.clone(),
        },
    )
    .await
    .map_err(OAuthError::Internal)?;

    session.set(OAUTH_PENDING_STATE_KEY, oauth_state.clone());
    session.set(OAUTH_PENDING_PROVIDER_KEY, provider.clone());

    let redirect_uri = settings.callback_url(&provider);
    let authorize_url = build_authorize_url(
        &endpoints,
        &credentials.client_id,
        &redirect_uri,
        &oauth_state,
        &pkce.code_challenge,
    );

    Ok(Redirect::temporary(&authorize_url))
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/oauth/{provider}/callback",
    tag = "Auth",
    summary = "OAuth コールバック",
    params(
        ("provider" = String, Path, description = "github | gitlab | gitlab_selfhosted | google | oidc"),
        ("code" = String, Query, description = "認可コード"),
        ("state" = String, Query, description = "CSRF state")
    ),
    responses(
        (status = 302, description = "フロントエンドへリダイレクト"),
        OAuthErrors,
    )
)]
pub async fn oauth_callback(
    Path(provider): Path<String>,
    Query(query): Query<OAuthCallbackQuery>,
    session: Session<SessionRedisPool>,
    State(state): State<AppState>,
) -> Result<Redirect, OAuthError> {
    let settings = &state.oauth_settings;

    if !settings.is_provider_configured(&provider) {
        return Err(OAuthError::ProviderNotConfigured);
    }

    if let Some(error) = &query.error {
        warn!(
            oauth_error = %error,
            error_description = query.error_description.as_deref().unwrap_or(""),
            provider = %provider,
            "oauth provider returned authorization error"
        );
        session.remove(OAUTH_PENDING_STATE_KEY);
        session.remove(OAUTH_PENDING_PROVIDER_KEY);

        let error_redirect_after = if let Some(state_param) = &query.state {
            consume_state(&state.redis_client, state_param)
                .await
                .ok()
                .flatten()
                .map(|p| p.error_redirect_after)
                .unwrap_or_else(|| settings.default_redirect_path.clone())
        } else {
            settings.default_redirect_path.clone()
        };

        let frontend_redirect = build_frontend_oauth_error_redirect(
            &state.settings.email_verification_app_url,
            &error_redirect_after,
            settings,
        )
        .map_err(|e| {
            warn!("oauth error redirect build failed: {e}");
            OAuthError::SecurityViolation
        })?;
        return Ok(Redirect::temporary(&frontend_redirect));
    }

    let code = query.code.ok_or(OAuthError::BadRequest)?;
    let oauth_state_param = query.state.ok_or(OAuthError::BadRequest)?;

    let pending_state: Option<String> = session.get(OAUTH_PENDING_STATE_KEY);
    let pending_provider: Option<String> = session.get(OAUTH_PENDING_PROVIDER_KEY);
    if pending_state.as_deref() != Some(oauth_state_param.as_str())
        || pending_provider.as_deref() != Some(provider.as_str())
    {
        return Err(OAuthError::InvalidState);
    }
    session.remove(OAUTH_PENDING_STATE_KEY);
    session.remove(OAUTH_PENDING_PROVIDER_KEY);

    let payload = consume_state(&state.redis_client, &oauth_state_param)
        .await
        .map_err(OAuthError::Internal)?
        .ok_or(OAuthError::InvalidState)?;

    if payload.provider != provider {
        return Err(OAuthError::InvalidState);
    }

    if let Some(link_user_id) = payload.link_user_id {
        let session_user_id: Option<Uuid> = session.get("user_id");
        if session_user_id != Some(link_user_id) {
            return Err(OAuthError::InvalidState);
        }
    }

    let instance_url = payload.instance_url.as_deref();
    let endpoints = resolve_endpoints(&provider, settings, instance_url, &state.http_client)
        .await
        .map_err(OAuthError::Internal)?;
    let credentials = get_credentials(&provider, settings).map_err(OAuthError::Internal)?;
    let redirect_uri = settings.callback_url(&provider);

    let token = exchange_code(
        &state.http_client,
        &endpoints,
        &credentials,
        &code,
        &redirect_uri,
        &payload.code_verifier,
    )
    .await
    .map_err(OAuthError::Internal)?;

    let provider_info = fetch_user_info(
        &state.http_client,
        &provider,
        &endpoints,
        &token.access_token,
    )
    .await
    .map_err(OAuthError::Internal)?;

    let db_provider = settings
        .db_provider_key(&provider)
        .ok_or(OAuthError::ProviderNotConfigured)?;

    let user_id = resolve_user_and_connection(
        &state,
        &provider,
        &db_provider,
        instance_url,
        &provider_info,
        &token,
        payload.link_user_id,
    )
    .await?;

    let user_model = users::Entity::find_by_id(user_id)
        .one(&state.db)
        .await?
        .ok_or(OAuthError::Unauthorized)?;

    if user_model.is_suspended {
        return Err(OAuthError::AccountSuspended);
    }

    let twofa_required = establish_login_session(&session, &state.db, &user_model)
        .await
        .map_err(OAuthError::from)?;

    if twofa_required.is_some() {
        let twofa_redirect = format!(
            "{}/auth/2fa?redirect_after={}",
            state
                .settings
                .email_verification_app_url
                .trim_end_matches('/'),
            urlencoding::encode(&payload.redirect_after)
        );
        return Ok(Redirect::temporary(&twofa_redirect));
    }

    // フロント基底 URL は email 認証と同一（単一フロント前提）。OAuth 専用 URL は未分離。
    // email_verification_app_url is the configured frontend base URL (shared with verification emails).
    let frontend_redirect = build_frontend_redirect(
        &state.settings.email_verification_app_url,
        &payload.redirect_after,
        settings,
    )
    .map_err(|e| {
        warn!("oauth success redirect build failed: {e}");
        OAuthError::SecurityViolation
    })?;

    Ok(Redirect::temporary(&frontend_redirect))
}

/// discovery で列挙する OAuth プロバイダー slug と、self-hosted インスタンス URL 入力の要否。
const OAUTH_PROVIDER_SLUGS: [(&str, bool); 5] = [
    ("github", false),
    ("gitlab", false),
    ("gitlab_selfhosted", true),
    ("google", false),
    ("oidc", false),
];

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/oauth/providers",
    tag = "Auth",
    summary = "有効な OAuth プロバイダー一覧",
    responses(
        (status = 200, description = "有効な OAuth プロバイダー一覧", body = OAuthProvidersResponse),
    )
)]
pub async fn list_providers(State(state): State<AppState>) -> Json<OAuthProvidersResponse> {
    let settings = &state.oauth_settings;
    let providers = OAUTH_PROVIDER_SLUGS
        .iter()
        .filter(|(slug, _)| settings.is_provider_configured(slug))
        .map(|(slug, requires_instance_url)| OAuthProviderItem {
            provider: (*slug).to_string(),
            requires_instance_url: *requires_instance_url,
        })
        .collect();

    Json(OAuthProvidersResponse { providers })
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/oauth/connections",
    tag = "Auth",
    summary = "連携済み OAuth プロバイダー一覧",
    responses(
        (status = 200, description = "連携一覧", body = OAuthConnectionsResponse),
        OAuthErrors,
    )
)]
pub async fn list_connections(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<OAuthConnectionsResponse>, OAuthError> {
    let rows = oauth_connections::Entity::find()
        .filter(oauth_connections::Column::UserId.eq(user.id))
        .all(&state.db)
        .await?;

    let connections = rows
        .into_iter()
        .map(|row| OAuthConnectionItem {
            provider: row.provider,
            provider_email: row.provider_email,
            instance_url: row.instance_url,
            connected_at: row.created_at.to_rfc3339(),
        })
        .collect();

    Ok(Json(OAuthConnectionsResponse { connections }))
}

#[axum::debug_handler]
#[utoipa::path(
    delete,
    path = "/oauth/connections/{provider}",
    tag = "Auth",
    summary = "OAuth 連携解除",
    params(
        ("provider" = String, Path, description = "プロバイダー slug または DB provider キー"),
        ("instance_url" = Option<String>, Query, description = "GitLab self-hosted インスタンス URL")
    ),
    responses(
        (status = 204, description = "連携を解除しました"),
        OAuthErrors,
    )
)]
pub async fn disconnect_connection(
    Path(provider): Path<String>,
    Query(query): Query<DisconnectQuery>,
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<StatusCode, OAuthError> {
    auth.require_session()
        .map_err(|_| OAuthError::Unauthorized)?;

    let db_provider = resolve_disconnect_provider(&state.oauth_settings, &provider)?;

    if db_provider == "gitlab_selfhosted" && query.instance_url.is_none() {
        return Err(OAuthError::BadRequest);
    }

    let mut filter = oauth_connections::Entity::find()
        .filter(oauth_connections::Column::UserId.eq(auth.user_id))
        .filter(oauth_connections::Column::Provider.eq(&db_provider));

    if let Some(instance_url) = query.instance_url.as_deref() {
        let normalized = normalize_instance_url(instance_url).map_err(OAuthError::Internal)?;
        filter = filter.filter(oauth_connections::Column::InstanceUrl.eq(normalized));
    }

    with_transaction::<(), OAuthError, _>(&state.db, |txn| {
        Box::pin(async move {
            let user = users::Entity::find_by_id(auth.user_id)
                .lock_exclusive()
                .one(txn)
                .await?
                .ok_or(OAuthError::Unauthorized)?;

            let connection = filter
                .one(txn)
                .await?
                .ok_or(OAuthError::ConnectionNotFound)?;

            let connection_count = oauth_connections::Entity::find()
                .filter(oauth_connections::Column::UserId.eq(auth.user_id))
                .count(txn)
                .await?;

            let passkey_count = count_user_passkeys(txn, auth.user_id)
                .await
                .map_err(|e| OAuthError::Internal(e.into()))?;
            if connection_count <= 1 && user.password_hash.is_none() && passkey_count == 0 {
                return Err(OAuthError::LastAuthMethod);
            }

            oauth_connections::Entity::delete_by_id(connection.id)
                .exec(txn)
                .await?;

            Ok(())
        })
    })
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/password",
    tag = "Auth",
    summary = "OAuth ユーザーの初回パスワード設定",
    request_body = SetPasswordRequest,
    responses(
        (status = 204, description = "パスワードを設定しました"),
        OAuthErrors,
    )
)]
pub async fn set_initial_password(
    State(state): State<AppState>,
    auth: AuthUser,
    Valid(Json(payload)): Valid<Json<SetPasswordRequest>>,
) -> Result<StatusCode, OAuthError> {
    auth.require_session()
        .map_err(|_| OAuthError::Unauthorized)?;

    let user = users::Entity::find_by_id(auth.user_id)
        .one(&state.db)
        .await?
        .ok_or(OAuthError::Unauthorized)?;

    if user.password_hash.is_some() {
        return Err(OAuthError::PasswordAlreadySet);
    }

    let password_hash = create_password_hash(&payload.password).map_err(OAuthError::from)?;

    let mut active: users::ActiveModel = user.into();
    active.password_hash = Set(Some(password_hash));
    active.update(&state.db).await?;

    Ok(StatusCode::NO_CONTENT)
}

async fn resolve_user_and_connection(
    state: &AppState,
    provider_slug: &str,
    db_provider: &str,
    instance_url: Option<&str>,
    provider_info: &ProviderUserInfo,
    token: &TokenResponse,
    link_user_id: Option<Uuid>,
) -> Result<Uuid, OAuthError> {
    let normalized_instance = match instance_url {
        Some(url) => Some(normalize_instance_url(url).map_err(OAuthError::Internal)?),
        None => None,
    };

    let existing = find_connection(
        &state.db,
        db_provider,
        &provider_info.provider_user_id,
        normalized_instance.as_deref(),
    )
    .await?;

    if let Some(conn) = existing {
        if let Some(expected_user_id) = link_user_id
            && conn.user_id != expected_user_id
        {
            return Err(OAuthError::ConnectionExists);
        }
        let user_id = conn.user_id;
        update_connection_tokens(state, conn, token).await?;
        return Ok(user_id);
    }

    if let Some(link_user_id) = link_user_id {
        ensure_no_provider_conflict(
            &state.db,
            link_user_id,
            db_provider,
            normalized_instance.as_deref(),
        )
        .await?;
        insert_connection(
            state,
            link_user_id,
            db_provider,
            normalized_instance.as_deref(),
            provider_info,
            token,
        )
        .await?;
        return Ok(link_user_id);
    }

    if let Some(email) = provider_info.email.as_deref() {
        let normalized = normalize_email(email);
        if users::Entity::find()
            .filter(users::Column::Email.eq(normalized))
            .one(&state.db)
            .await?
            .is_some()
        {
            return Err(OAuthError::EmailConflict);
        }
    }

    create_oauth_user_and_connection(
        state,
        provider_slug,
        db_provider,
        normalized_instance.as_deref(),
        provider_info,
        token,
    )
    .await
}

async fn find_connection(
    db: &sea_orm::DatabaseConnection,
    provider: &str,
    provider_user_id: &str,
    instance_url: Option<&str>,
) -> Result<Option<oauth_connections::Model>, OAuthError> {
    let mut query = oauth_connections::Entity::find()
        .filter(oauth_connections::Column::Provider.eq(provider))
        .filter(oauth_connections::Column::ProviderUserId.eq(provider_user_id));

    query = match instance_url {
        Some(url) => query.filter(oauth_connections::Column::InstanceUrl.eq(url)),
        None => query.filter(oauth_connections::Column::InstanceUrl.is_null()),
    };

    query.one(db).await.map_err(OAuthError::from)
}

async fn ensure_no_provider_conflict(
    db: &sea_orm::DatabaseConnection,
    user_id: Uuid,
    provider: &str,
    instance_url: Option<&str>,
) -> Result<(), OAuthError> {
    let mut query = oauth_connections::Entity::find()
        .filter(oauth_connections::Column::UserId.eq(user_id))
        .filter(oauth_connections::Column::Provider.eq(provider));

    query = match instance_url {
        Some(url) => query.filter(oauth_connections::Column::InstanceUrl.eq(url)),
        None => query.filter(oauth_connections::Column::InstanceUrl.is_null()),
    };

    if query.one(db).await?.is_some() {
        return Err(OAuthError::ConnectionExists);
    }

    Ok(())
}

async fn insert_connection(
    state: &AppState,
    user_id: Uuid,
    provider: &str,
    instance_url: Option<&str>,
    provider_info: &ProviderUserInfo,
    token: &TokenResponse,
) -> Result<(), OAuthError> {
    insert_connection_txn(
        state,
        &state.db,
        user_id,
        provider,
        instance_url,
        provider_info,
        token,
    )
    .await
}

async fn insert_connection_txn(
    state: &AppState,
    db: &impl sea_orm::ConnectionTrait,
    user_id: Uuid,
    provider: &str,
    instance_url: Option<&str>,
    provider_info: &ProviderUserInfo,
    token: &TokenResponse,
) -> Result<(), OAuthError> {
    let access_token_enc = encrypt_token(&state.oauth_settings.encryption_key, &token.access_token)
        .map_err(OAuthError::Internal)?;
    let refresh_token_enc = match &token.refresh_token {
        Some(rt) => Some(
            encrypt_token(&state.oauth_settings.encryption_key, rt)
                .map_err(OAuthError::Internal)?,
        ),
        None => None,
    };

    let now = Utc::now();
    let connection = oauth_connections::ActiveModel {
        id: Set(Uuid::new_v4()),
        user_id: Set(user_id),
        provider: Set(provider.to_string()),
        provider_user_id: Set(provider_info.provider_user_id.clone()),
        provider_email: Set(provider_info.email.clone()),
        instance_url: Set(instance_url.map(str::to_string)),
        access_token_enc: Set(Some(access_token_enc)),
        refresh_token_enc: Set(refresh_token_enc),
        token_expires_at: Set(token.expires_at.map(Into::into)),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
    };

    oauth_connections::Entity::insert(connection)
        .exec(db)
        .await
        .map_err(|e| {
            if is_postgres_unique_violation(&e) {
                OAuthError::ConnectionExists
            } else {
                OAuthError::Internal(e.into())
            }
        })?;

    Ok(())
}

async fn update_connection_tokens(
    state: &AppState,
    conn: oauth_connections::Model,
    token: &TokenResponse,
) -> Result<(), OAuthError> {
    let access_token_enc = encrypt_token(&state.oauth_settings.encryption_key, &token.access_token)
        .map_err(OAuthError::Internal)?;
    let refresh_token_enc = match &token.refresh_token {
        Some(rt) => Some(
            encrypt_token(&state.oauth_settings.encryption_key, rt)
                .map_err(OAuthError::Internal)?,
        ),
        None => conn.refresh_token_enc.clone(),
    };

    let mut active: oauth_connections::ActiveModel = conn.into();
    active.access_token_enc = Set(Some(access_token_enc));
    active.refresh_token_enc = Set(refresh_token_enc);
    active.token_expires_at = Set(token.expires_at.map(Into::into));
    active.updated_at = Set(Utc::now().into());
    active.update(&state.db).await?;

    Ok(())
}

async fn create_oauth_user_and_connection(
    state: &AppState,
    _provider_slug: &str,
    db_provider: &str,
    instance_url: Option<&str>,
    provider_info: &ProviderUserInfo,
    token: &TokenResponse,
) -> Result<Uuid, OAuthError> {
    let user_id = Uuid::new_v4();
    let username = derive_unique_username(&state.db, &provider_info.username).await?;
    let email = provider_info
        .email
        .as_ref()
        .map(|e| normalize_email(e))
        .unwrap_or_else(|| format!("{user_id}@oauth.local"));

    let access_token_enc = encrypt_token(&state.oauth_settings.encryption_key, &token.access_token)
        .map_err(OAuthError::Internal)?;
    let refresh_token_enc = match &token.refresh_token {
        Some(rt) => Some(
            encrypt_token(&state.oauth_settings.encryption_key, rt)
                .map_err(OAuthError::Internal)?,
        ),
        None => None,
    };
    let now = Utc::now();

    let user = users::ActiveModel {
        id: Set(user_id),
        username: Set(username.clone()),
        bio: Set(Some(String::new())),
        avatar_url: Set(provider_info.avatar_url.clone()),
        email: Set(email.clone()),
        email_verified: Set(provider_info.email_verified.unwrap_or(false)),
        password_hash: Set(None),
        is_admin: Set(false),
        is_suspended: Set(false),
        ..Default::default()
    };

    let connection = oauth_connections::ActiveModel {
        id: Set(Uuid::new_v4()),
        user_id: Set(user_id),
        provider: Set(db_provider.to_string()),
        provider_user_id: Set(provider_info.provider_user_id.clone()),
        provider_email: Set(provider_info.email.clone()),
        instance_url: Set(instance_url.map(str::to_string)),
        access_token_enc: Set(Some(access_token_enc)),
        refresh_token_enc: Set(refresh_token_enc),
        token_expires_at: Set(token.expires_at.map(Into::into)),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
    };

    with_transaction::<Uuid, OAuthError, _>(&state.db, |txn| {
        Box::pin(async move {
            if let Err(e) = users::Entity::insert(user).exec(txn).await {
                if is_postgres_unique_violation(&e) {
                    return Err(classify_user_unique_violation(txn, &email, &username).await?);
                }
                return Err(OAuthError::Internal(e.into()));
            }

            oauth_connections::Entity::insert(connection)
                .exec(txn)
                .await
                .map_err(|e| {
                    if is_postgres_unique_violation(&e) {
                        OAuthError::ConnectionExists
                    } else {
                        OAuthError::Internal(e.into())
                    }
                })?;

            Ok(user_id)
        })
    })
    .await
}

async fn derive_unique_username(
    db: &sea_orm::DatabaseConnection,
    base: &str,
) -> Result<String, OAuthError> {
    let sanitized: String = base
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .take(50)
        .collect();

    let base = if sanitized.is_empty() {
        "oauth_user".to_string()
    } else {
        sanitized
    };

    for i in 0..100 {
        let candidate = if i == 0 {
            base.clone()
        } else {
            format!("{base}_{}", i + 1)
        };

        let exists = users::Entity::find()
            .filter(users::Column::Username.eq(&candidate))
            .one(db)
            .await?
            .is_some();

        if !exists {
            return Ok(candidate);
        }
    }

    Err(OAuthError::Internal(anyhow::anyhow!(
        "failed to derive unique username"
    )))
}

async fn classify_user_unique_violation(
    db: &impl sea_orm::ConnectionTrait,
    email: &str,
    username: &str,
) -> Result<OAuthError, OAuthError> {
    if users::Entity::find()
        .filter(users::Column::Email.eq(email))
        .one(db)
        .await?
        .is_some()
    {
        return Ok(OAuthError::EmailConflict);
    }

    if users::Entity::find()
        .filter(users::Column::Username.eq(username))
        .one(db)
        .await?
        .is_some()
    {
        return Ok(OAuthError::UsernameConflict);
    }

    Ok(OAuthError::Internal(anyhow::anyhow!(
        "unexpected unique constraint on user insert"
    )))
}

fn resolve_disconnect_provider(
    settings: &github_integration::oauth::OAuthSettings,
    provider: &str,
) -> Result<String, OAuthError> {
    if provider == "oidc" {
        return settings
            .db_provider_key("oidc")
            .ok_or(OAuthError::ProviderNotConfigured);
    }

    if settings.is_provider_configured(provider) {
        return settings
            .db_provider_key(provider)
            .ok_or(OAuthError::ProviderNotConfigured);
    }

    Ok(provider.to_string())
}
