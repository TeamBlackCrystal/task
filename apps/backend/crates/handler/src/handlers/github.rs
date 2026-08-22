use axum::{
    Json,
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use hmac::{Hmac, KeyInit, Mac};
use sea_orm::prelude::Uuid;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, TransactionTrait,
};
use sha2::Sha256;
use subtle::ConstantTimeEq;

use crate::AppState;
use crate::error::AppError;
use crate::extractors::AuthUser;
use crate::openapi::CrudErrors;
use crate::settings::GithubAppSettings;
use entity::{github_integrations, github_issue_links, projects, tenants};
use job::github_issue_sync::{self, GithubIssueSyncJob};
use job::github_webhook::{self, GithubWebhookJob};
use payload::github::*;
use service::github::{
    github_app,
    install_state::{self, GithubOAuthStatePayload, TTL_SECS},
    repositories::fetch_primary_repository,
};

type HmacSha256 = Hmac<Sha256>;

async fn require_tenant_owner(
    state: &AppState,
    tenant_id: Uuid,
    user_id: Uuid,
) -> Result<(), AppError> {
    let tenant = tenants::Entity::find_by_id(tenant_id)
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    if tenant.owner_id != user_id {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

async fn require_project_in_tenant(
    state: &AppState,
    tenant_id: Uuid,
    project_id: Uuid,
) -> Result<(), AppError> {
    let exists = projects::Entity::find_by_id(project_id)
        .filter(projects::Column::TenantId.eq(tenant_id))
        .one(&state.db)
        .await?
        .is_some();
    if exists {
        Ok(())
    } else {
        Err(AppError::NotFound)
    }
}

fn install_redirect_url(github: &GithubAppSettings, state: &str) -> String {
    format!(
        "https://github.com/apps/{}/installations/new?state={}",
        github.github_app_name, state
    )
}

/// インストール完了後に戻す frontend の設定ページ URL。
/// frontend のルートは display_id / プロジェクトキー基準（`/{display_id}/projects/{key}/settings`）
/// のため、UUID から引き直して組み立てる（UUID 直書きの旧 URL は実在せず 404 になっていた）。
async fn settings_redirect_url(
    db: &sea_orm::DatabaseConnection,
    github: &GithubAppSettings,
    tenant_id: Uuid,
    project_id: Uuid,
) -> Result<String, AppError> {
    let tenant = tenants::Entity::find_by_id(tenant_id)
        .one(db)
        .await?
        .ok_or(AppError::NotFound)?;
    let project = projects::Entity::find_by_id(project_id)
        .one(db)
        .await?
        .ok_or(AppError::NotFound)?;
    let base = github.github_app_frontend_base_url.trim_end_matches('/');
    Ok(format!(
        "{base}/{}/projects/{}/settings?section=integrations",
        tenant.display_id, project.key
    ))
}

/// GitHub Webhook 署名検証（HMAC-SHA256, ConstantTimeEq）。
pub fn verify_webhook_signature(secret: &str, signature_header: &str, body: &[u8]) -> bool {
    let Some(hex_digest) = signature_header.strip_prefix("sha256=") else {
        return false;
    };
    let Ok(expected) = hex::decode(hex_digest) else {
        return false;
    };
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
    mac.update(body);
    let computed = mac.finalize().into_bytes();
    expected.ct_eq(computed.as_slice()).into()
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/install",
    tag = "GitHub",
    summary = "GitHub App インストール URL 取得",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
    ),
    responses(
        (status = 200, body = GithubInstallUrlResponse, description = "GitHub インストール URL"),
        CrudErrors,
    )
)]
pub async fn start_github_install(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<GithubInstallUrlResponse>, AppError> {
    let github = state.settings.require_github_app()?;
    auth.require_session()?;
    require_tenant_owner(&state, tenant_id, auth.user_id).await?;
    require_project_in_tenant(&state, tenant_id, project_id).await?;

    let existing_installation_id = github_integrations::Entity::find()
        .filter(github_integrations::Column::ProjectId.eq(project_id))
        .one(&state.db)
        .await?
        .map(|row| row.installation_id);

    let state_token = install_state::new_state_token();
    install_state::store_state(
        &state.redis_client,
        &state_token,
        &GithubOAuthStatePayload {
            tenant_id,
            project_id,
            user_id: auth.user_id,
            installation_id: existing_installation_id,
        },
    )
    .await
    .map_err(AppError::Internal)?;

    let url = install_redirect_url(github, &state_token);
    Ok(Json(GithubInstallUrlResponse { url }))
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/callback",
    tag = "GitHub",
    summary = "GitHub App インストールコールバック",
    params(GithubCallbackQuery),
    responses(
        (status = 302, description = "設定画面へリダイレクト"),
        (status = 400, description = "無効な state / setup_action=request"),
        (status = 403, description = "ユーザー不一致"),
    )
)]
pub async fn github_callback(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<GithubCallbackQuery>,
) -> Result<Response, AppError> {
    let github = state.settings.require_github_app()?;
    auth.require_session()?;

    // setup_action=request はオーナーへの承認リクエスト段階。インストール未完了なので拒否。
    if query.setup_action.as_deref() == Some("request") {
        tracing::info!(
            installation_id = query.installation_id,
            "github callback: setup_action=request, installation pending owner approval"
        );
        return Err(AppError::BadRequest);
    }

    let payload = install_state::consume_state(&state.redis_client, &query.state)
        .await
        .map_err(AppError::Internal)?
        .ok_or(AppError::BadRequest)?;

    if payload.user_id != auth.user_id {
        return Err(AppError::Forbidden);
    }

    require_tenant_owner(&state, payload.tenant_id, auth.user_id).await?;
    require_project_in_tenant(&state, payload.tenant_id, payload.project_id).await?;

    let app = github_app(&state.http_client, github);
    // 新規インストールは state の TTL 内に作成されたものだけ受け付ける（古い
    // installation_id を差し込む攻撃を防ぐ）。再連携時は state に束縛済みの ID と照合する。
    let installation = app
        .verify_installation(
            query.installation_id,
            payload.installation_id,
            chrono::Duration::seconds(TTL_SECS as i64),
        )
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "github callback installation verification failed");
            AppError::BadRequest
        })?;

    let access = app
        .installation_access_token(installation.id)
        .await
        .map_err(AppError::Internal)?;
    let account_login = installation.account_login;
    let (repo_owner, repo_name) = fetch_primary_repository(&app, &access.token, &account_login)
        .await
        .map_err(AppError::Internal)?;

    let token_enc =
        auth_core::crypto::encrypt_token(&github.github_token_encryption_key, &access.token)
            .map_err(AppError::Internal)?;

    let now = chrono::Utc::now();
    let existing = github_integrations::Entity::find()
        .filter(github_integrations::Column::ProjectId.eq(payload.project_id))
        .one(&state.db)
        .await?;

    if let Some(model) = existing {
        // 再連携: created_by / created_at は変更しない
        let repo_changed = model.repo_owner != repo_owner || model.repo_name != repo_name;
        let txn = state.db.begin().await?;
        if repo_changed {
            // 旧リポジトリの Issue に紐づくリンクを残すと、書き戻しや再インポートが
            // 新リポジトリの同番号 Issue を上書きする。連携先変更と同一トランザクションで消す。
            github_issue_links::Entity::delete_many()
                .filter(github_issue_links::Column::ProjectId.eq(payload.project_id))
                .exec(&txn)
                .await?;
        }
        let mut active: github_integrations::ActiveModel = model.into();
        active.installation_id = Set(query.installation_id);
        active.repo_owner = Set(repo_owner);
        active.repo_name = Set(repo_name);
        active.access_token_enc = Set(token_enc);
        active.token_expires_at = Set(access.expires_at);
        active.update(&txn).await?;
        txn.commit().await?;
    } else {
        github_integrations::ActiveModel {
            id: Set(Uuid::new_v4()),
            project_id: Set(payload.project_id),
            installation_id: Set(query.installation_id),
            repo_owner: Set(repo_owner),
            repo_name: Set(repo_name),
            access_token_enc: Set(token_enc),
            token_expires_at: Set(access.expires_at),
            created_by: Set(auth.user_id),
            created_at: Set(now.into()),
        }
        .insert(&state.db)
        .await?;
    }

    let redirect_to =
        settings_redirect_url(&state.db, github, payload.tenant_id, payload.project_id).await?;
    Ok(Redirect::temporary(&redirect_to).into_response())
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/webhook",
    tag = "GitHub",
    summary = "GitHub Webhook 受信",
    responses(
        (status = 200, description = "受信成功"),
        (status = 403, description = "署名不一致"),
    )
)]
pub async fn github_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, AppError> {
    let github = state.settings.require_github_app()?;
    let signature = headers
        .get("X-Hub-Signature-256")
        .and_then(|v| v.to_str().ok())
        .ok_or(AppError::Forbidden)?;
    if !verify_webhook_signature(&github.github_app_webhook_secret, signature, &body) {
        return Err(AppError::Forbidden);
    }

    let event = headers
        .get("X-GitHub-Event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();
    let delivery_id = headers
        .get("X-GitHub-Delivery")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    let payload: serde_json::Value =
        serde_json::from_slice(&body).map_err(|_| AppError::BadRequest)?;

    let installation_id = payload
        .get("installation")
        .and_then(|i| i.get("id"))
        .and_then(|id| id.as_i64())
        .or_else(|| payload.get("installation_id").and_then(|id| id.as_i64()));

    // payload に repository が含まれる場合はリポジトリ単位で絞り込む。
    // installation 全体イベント (installation, installation_repositories 等) は
    // repository フィールドを持たないため、その場合は installation 配下の全件を対象とする。
    let repo_filter: Option<(String, String)> = payload.get("repository").and_then(|r| {
        let owner = r.get("owner")?.get("login")?.as_str()?.to_owned();
        let name = r.get("name")?.as_str()?.to_owned();
        Some((owner, name))
    });

    if let Some(installation_id) = installation_id {
        let mut query = github_integrations::Entity::find()
            .filter(github_integrations::Column::InstallationId.eq(installation_id));

        if let Some((ref owner, ref name)) = repo_filter {
            query = query
                .filter(github_integrations::Column::RepoOwner.eq(owner.as_str()))
                .filter(github_integrations::Column::RepoName.eq(name.as_str()));
        }

        let integrations = query.all(&state.db).await?;

        if integrations.is_empty() {
            tracing::warn!(
                installation_id,
                event = %event,
                delivery_id = ?delivery_id,
                repo = ?repo_filter,
                "github webhook: no integration found"
            );
        }

        let jobs: Vec<GithubWebhookJob> = integrations
            .into_iter()
            .map(|integration| GithubWebhookJob {
                integration_id: integration.id,
                project_id: integration.project_id,
                event: event.clone(),
                delivery_id: delivery_id.clone(),
                payload: payload.clone(),
            })
            .collect();

        // TODO(#9b): Wave 1+ で delivery_id + integration_id ベースの重複排除を追加
        for job in jobs {
            github_webhook::enqueue(&state.github_webhook_storage, job)
                .await
                .map_err(AppError::Internal)?;
        }
    }

    Ok(StatusCode::OK)
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/integration",
    tag = "GitHub",
    summary = "GitHub 連携状態取得",
    params(
        ("tenant_id" = Uuid, Path),
        ("project_id" = Uuid, Path),
    ),
    responses((status = 200, body = GithubIntegrationResponse), CrudErrors)
)]
pub async fn get_github_integration(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<GithubIntegrationResponse>, AppError> {
    state.settings.require_github_app()?;
    auth.require_session()?;
    require_tenant_owner(&state, tenant_id, auth.user_id).await?;
    require_project_in_tenant(&state, tenant_id, project_id).await?;

    let integration = github_integrations::Entity::find()
        .filter(github_integrations::Column::ProjectId.eq(project_id))
        .one(&state.db)
        .await?;

    Ok(Json(match integration {
        Some(row) => GithubIntegrationResponse {
            connected: true,
            repo_owner: Some(row.repo_owner),
            repo_name: Some(row.repo_name),
            connected_at: Some(row.created_at.with_timezone(&chrono::Utc)),
        },
        None => GithubIntegrationResponse {
            connected: false,
            repo_owner: None,
            repo_name: None,
            connected_at: None,
        },
    }))
}

#[axum::debug_handler]
#[utoipa::path(
    delete,
    path = "/integration",
    tag = "GitHub",
    summary = "GitHub 連携解除",
    params(
        ("tenant_id" = Uuid, Path),
        ("project_id" = Uuid, Path),
    ),
    responses((status = 204, description = "解除完了"), CrudErrors)
)]
pub async fn delete_github_integration(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    let github = state.settings.require_github_app()?;
    auth.require_session()?;
    require_tenant_owner(&state, tenant_id, auth.user_id).await?;
    require_project_in_tenant(&state, tenant_id, project_id).await?;

    let integration = github_integrations::Entity::find()
        .filter(github_integrations::Column::ProjectId.eq(project_id))
        .one(&state.db)
        .await?;

    let Some(row) = integration else {
        return Err(AppError::NotFound);
    };

    let installation_id = row.installation_id;

    // GitHub 側を先に解除する（404/410 は冪等成功として delete_installation 内で処理済み）。
    // DB を先に削除すると GitHub 側の失敗時に installation_id が失われ再試行不能になる。
    github_app(&state.http_client, github)
        .delete_installation(installation_id)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, installation_id, "github delete_installation failed");
            AppError::Internal(e)
        })?;

    let active: github_integrations::ActiveModel = row.into();
    active.delete(&state.db).await?;

    Ok(StatusCode::NO_CONTENT)
}

/// 書き戻しジョブを積む（ベストエフォート）。
///
/// 呼び出し時点で書き戻し要求はタスク更新と同じトランザクション内の
/// `pending_push`（`service::github::sync::mark_pending_push`）として永続化済み。
/// ここでの登録失敗は API エラーにせず、定期スイープに回収を任せる。
pub(crate) async fn enqueue_issue_push(state: &AppState, task_id: Uuid) {
    if !state.settings.github_app_enabled() {
        return;
    }
    if let Err(e) = github_issue_sync::enqueue(
        &state.github_issue_sync_storage,
        GithubIssueSyncJob::Push { task_id },
    )
    .await
    {
        tracing::warn!(error = %e, %task_id, "enqueue github issue push failed; sweep will retry");
    }
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/import",
    tag = "GitHub",
    summary = "GitHub Issue の取り込みを開始",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
    ),
    responses(
        (status = 202, description = "取り込みジョブを登録しました"),
        CrudErrors,
    )
)]
pub async fn import_github_issues(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    state.settings.require_github_app()?;
    auth.require_session()?;
    require_tenant_owner(&state, tenant_id, auth.user_id).await?;
    require_project_in_tenant(&state, tenant_id, project_id).await?;

    github_integrations::Entity::find()
        .filter(github_integrations::Column::ProjectId.eq(project_id))
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    // 同じプロジェクトの取り込みが待機中・実行中なら積み直さない。
    // Redis に触れない場合は握り潰して従来どおり積む（重複防止は最適化であって
    // 認可ではないため、ここで API を落とさない）
    let acquired = match service::github::try_acquire_import_slot(&state.redis_client, project_id)
        .await
    {
        Ok(acquired) => acquired,
        Err(e) => {
            tracing::warn!(error = %e, %project_id, "github import lock unavailable; enqueueing anyway");
            true
        }
    };

    if !acquired {
        tracing::info!(%project_id, "github import already queued; skipping duplicate enqueue");
        return Ok(StatusCode::ACCEPTED);
    }

    if let Err(e) = github_issue_sync::enqueue(
        &state.github_issue_sync_storage,
        GithubIssueSyncJob::Import { project_id },
    )
    .await
    {
        // 積めなかったのにロックだけ残ると、TTL のあいだやり直せなくなる
        if let Err(release_err) =
            service::github::release_import_slot(&state.redis_client, project_id).await
        {
            tracing::warn!(error = %release_err, %project_id, "release github import lock failed");
        }
        return Err(AppError::Internal(e));
    }

    Ok(StatusCode::ACCEPTED)
}
