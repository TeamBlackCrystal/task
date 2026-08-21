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
    install_state::{self, GithubOAuthStatePayload, RepoSelectPayload, TTL_SECS},
    repositories::{contains_repository, select_primary_repository},
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

/// callback の失敗を、設定画面へ理由付きで戻すリダイレクトにする。
///
/// callback は GitHub からの着地点なので、素のエラーを返すとユーザーは何も分からない
/// GitHub のページに取り残される。理由コードは frontend が文言に落とす。
///
/// 呼び出し側がそのまま `return` できるよう、ハンドラーの戻り値の形で返す。
fn callback_error_redirect(redirect_to: &str, reason: &str) -> Result<Response, AppError> {
    Ok(Redirect::temporary(&format!("{redirect_to}&github_error={reason}")).into_response())
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
    // GitHub からの着地点なので、以降の失敗は素のエラーではなく設定画面へ理由付きで戻す。
    let redirect_to =
        settings_redirect_url(&state.db, github, payload.tenant_id, payload.project_id).await?;

    // 新規インストールは state の TTL 内に作成されたものだけ受け付ける（古い
    // installation_id を差し込む攻撃を防ぐ）。再連携時は state に束縛済みの ID と照合する。
    //
    // リポジトリ選択を放棄したインストールは、DB に行が無いまま古くなるため鮮度チェックに
    // 落ちる。そのプロジェクトの選択待ちとして控えてある ID と一致する場合だけ、束縛済みと
    // 同じ扱いにして通す。一致しなければ通常の新規インストール判定に戻すので、別の
    // インストールへ乗り換える動線を塞がない。
    let expected_installation_id = match payload.installation_id {
        Some(bound) => Some(bound),
        None => {
            let pending =
                install_state::peek_pending_installation(&state.redis_client, payload.project_id)
                    .await
                    .map_err(AppError::Internal)?
                    .filter(|pending| *pending == query.installation_id);
            match pending {
                Some(id) => Some(id),
                // 同じテナントの別プロジェクトが既に使っているインストールなら、そのテナントが
                // 正規に入れたものだと分かっている。1 インストールに複数リポジトリが見える
                // 構成では「同じ org の別リポジトリを別プロジェクトへ」が普通に起きるので、
                // 鮮度チェックだけで弾くと 10 分を過ぎた時点で連携できなくなる。
                None if installation_used_in_tenant(
                    &state,
                    payload.tenant_id,
                    query.installation_id,
                )
                .await? =>
                {
                    Some(query.installation_id)
                }
                None => None,
            }
        }
    };

    // 所有者確認。新規に束縛するときだけ行う。
    //
    // installation_id はクエリで差し込めるので、これが無いと「他人が入れた
    // インストール」をそのまま自分のプロジェクトへ紐付けられる。インストール時の
    // ユーザー認可で GitHub が付ける code を交換し、そのユーザーが見えるインストールに
    // 当該 ID が含まれることを GitHub 自身に答えさせる。User インストールでも
    // Organization インストールでも同じ経路で確かめられる。
    //
    // 束縛先が既に決まっている（expected_installation_id が Some）ときは省く。
    // その束縛に入るのは「state に載せた連携済みの ID」「このプロジェクトの選択待ちの
    // 控え」「同じテナントが使用中のインストール」のいずれかで、どれもこの確認を通った
    // callback からしか生まれない。ここで code を要求すると、リポジトリを足して戻る・
    // 選び直すといった復旧の動線が、GitHub が code を付け直すかどうかに左右される。
    if expected_installation_id.is_none() {
        let Some(code) = query.code.as_deref() else {
            tracing::warn!(
                installation_id = query.installation_id,
                "github callback: no authorization code; enable user authorization on the app"
            );
            return callback_error_redirect(&redirect_to, "installation_authorization_required");
        };
        match app
            .verify_installation_access(code, query.installation_id)
            .await
        {
            Ok(true) => {}
            Ok(false) => {
                tracing::warn!(
                    installation_id = query.installation_id,
                    "github callback: installation does not belong to the authenticated user"
                );
                return callback_error_redirect(&redirect_to, "installation_forbidden");
            }
            // 拒否ではなく GitHub 側の不調。アンインストールを促さないよう理由を分ける。
            Err(e) => {
                tracing::warn!(error = %e, "github callback: installation ownership check failed");
                return callback_error_redirect(&redirect_to, "github_unavailable");
            }
        }
    }

    let installation = match app
        .verify_installation(
            query.installation_id,
            expected_installation_id,
            chrono::Duration::seconds(TTL_SECS as i64),
        )
        .await
    {
        Ok(installation) => installation,
        Err(e) => {
            tracing::warn!(error = %e, "github callback installation verification failed");
            // GitHub からの着地点なので、素のエラーではなく設定画面へ理由付きで戻す。
            // 選択を放棄したまま控えが切れたインストールもここに来る（対処は入れ直し）。
            // 一時障害でアンインストールを促さないよう、拒否と不調は分ける。
            let reason = if is_installation_rejected(&e) {
                "installation_rejected"
            } else {
                "github_unavailable"
            };
            return callback_error_redirect(&redirect_to, reason);
        }
    };

    // 検証は通っているので、ここで落ちても戻れるように控えておく
    // （控えが無いと、鮮度が切れたあと入れ直すまで連携できなくなる）。
    let unavailable = async |e: anyhow::Error| -> Result<Response, AppError> {
        tracing::warn!(error = %e, "github callback: github api unavailable");
        install_state::store_pending_installation(
            &state.redis_client,
            payload.project_id,
            query.installation_id,
        )
        .await
        .map_err(AppError::Internal)?;
        callback_error_redirect(&redirect_to, "github_unavailable")
    };
    let access = match app.installation_access_token(installation.id).await {
        Ok(access) => access,
        Err(e) => return unavailable(e).await,
    };
    let repositories = match app.list_repositories(&access.token).await {
        Ok(repositories) => repositories,
        Err(e) => return unavailable(e).await,
    };

    // 0 件は連携先を選びようがない（GitHub 側でリポジトリ選択を外した状態）。
    // GitHub からの着地点なので、素のエラーではなく設定画面へ理由付きで戻す。
    if repositories.is_empty() {
        // リポジトリを足して戻ってきたとき、鮮度チェックで弾かれないよう控えておく。
        install_state::store_pending_installation(
            &state.redis_client,
            payload.project_id,
            query.installation_id,
        )
        .await
        .map_err(AppError::Internal)?;
        tracing::warn!(
            installation_id = query.installation_id,
            "github callback: installation has no accessible repositories"
        );
        return callback_error_redirect(&redirect_to, "no_repositories");
    }

    // 複数見えるときは連携せず、選択トークンを発行して設定ページへ戻す。
    // installation_id はトークン側（Redis）に束縛し、リクエストでは受け取らない。
    let Some(repo) = select_primary_repository(&repositories) else {
        let select_token = install_state::new_state_token();
        install_state::store_select_token(
            &state.redis_client,
            &select_token,
            &RepoSelectPayload {
                tenant_id: payload.tenant_id,
                project_id: payload.project_id,
                user_id: auth.user_id,
                installation_id: query.installation_id,
            },
        )
        .await
        .map_err(AppError::Internal)?;
        install_state::store_pending_installation(
            &state.redis_client,
            payload.project_id,
            query.installation_id,
        )
        .await
        .map_err(AppError::Internal)?;
        // トークンはクエリではなくフラグメントに載せる。
        // フラグメントは次の HTTP リクエストに乗らないので、
        // frontend / CDN のアクセスログにトークンが残らない
        // （ブラウザ履歴には一時的に残るが、frontend が読んだ直後に URL から落とす）。
        return Ok(
            Redirect::temporary(&format!("{redirect_to}#github_select={select_token}"))
                .into_response(),
        );
    };

    upsert_integration(
        &state,
        github,
        payload.project_id,
        auth.user_id,
        query.installation_id,
        &repo.owner,
        &repo.name,
        &access,
    )
    .await?;
    // 連携は済んでいるので、後片付けの失敗で着地点をエラーにしない（TTL で切れる）。
    if let Err(e) = install_state::delete_pending_installation_if(
        &state.redis_client,
        payload.project_id,
        query.installation_id,
    )
    .await
    {
        tracing::warn!(error = %e, "discard pending github installation failed; TTL will expire it");
    }

    Ok(Redirect::temporary(&redirect_to).into_response())
}

/// 連携レコードの UPSERT。callback（自動選択）と選択確定 API で共有する。
#[allow(clippy::too_many_arguments)]
async fn upsert_integration(
    state: &AppState,
    github: &GithubAppSettings,
    project_id: Uuid,
    user_id: Uuid,
    installation_id: i64,
    repo_owner: &str,
    repo_name: &str,
    access: &forge_github::InstallationAccessToken,
) -> Result<(), AppError> {
    let token_enc =
        auth_core::crypto::encrypt_token(&github.github_token_encryption_key, &access.token)
            .map_err(AppError::Internal)?;

    let now = chrono::Utc::now();
    let existing = github_integrations::Entity::find()
        .filter(github_integrations::Column::ProjectId.eq(project_id))
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
                .filter(github_issue_links::Column::ProjectId.eq(project_id))
                .exec(&txn)
                .await?;
        }
        let mut active: github_integrations::ActiveModel = model.into();
        active.installation_id = Set(installation_id);
        active.repo_owner = Set(repo_owner.to_owned());
        active.repo_name = Set(repo_name.to_owned());
        active.access_token_enc = Set(token_enc);
        active.token_expires_at = Set(access.expires_at);
        active.update(&txn).await?;
        txn.commit().await?;
    } else {
        github_integrations::ActiveModel {
            id: Set(Uuid::new_v4()),
            project_id: Set(project_id),
            installation_id: Set(installation_id),
            repo_owner: Set(repo_owner.to_owned()),
            repo_name: Set(repo_name.to_owned()),
            access_token_enc: Set(token_enc),
            token_expires_at: Set(access.expires_at),
            created_by: Set(user_id),
            created_at: Set(now.into()),
        }
        .insert(&state.db)
        .await?;
    }

    Ok(())
}

/// そのテナントのどれかのプロジェクトが、既にこのインストールを使っているか。
async fn installation_used_in_tenant(
    state: &AppState,
    tenant_id: Uuid,
    installation_id: i64,
) -> Result<bool, AppError> {
    let used = github_integrations::Entity::find()
        .filter(github_integrations::Column::InstallationId.eq(installation_id))
        .inner_join(projects::Entity)
        .filter(projects::Column::TenantId.eq(tenant_id))
        .one(&state.db)
        .await?
        .is_some();
    Ok(used)
}

/// インストールが GitHub 側から消えているか。
///
/// ponytail: forge-github がステータスをエラー型に持たないため文字列で判定している。
/// 型で受け取れるようになったら差し替える。
fn is_installation_gone(error: &anyhow::Error) -> bool {
    // 403（サスペンド）はレート制限とも区別できないため、ここには含めない。
    let message = error.to_string();
    message.contains("failed: 404") || message.contains("failed: 410")
}

/// installation の検証が「拒否」なのか、GitHub 側の不調なのか。
///
/// ponytail: forge-github が理由を型で返さないため文字列で判定している。
fn is_installation_rejected(error: &anyhow::Error) -> bool {
    let message = error.to_string();
    message.contains("installation id")
        || message.contains("too old")
        || is_installation_gone(error)
}

/// 選択トークンを検証し、束縛された installation のリポジトリ一覧を取得する。
async fn resolve_select_token(
    state: &AppState,
    auth: &AuthUser,
    tenant_id: Uuid,
    project_id: Uuid,
    payload: Option<RepoSelectPayload>,
) -> Result<
    (
        RepoSelectPayload,
        Vec<forge_core::Repository>,
        forge_github::InstallationAccessToken,
    ),
    AppError,
> {
    let github = state.settings.require_github_app()?;
    let payload = payload.ok_or(AppError::BadRequest)?;
    // トークンは tenant / project / user / installation に束縛されている。
    // 経路（パス）とずれていたら他人のインストールを紐付ける試みなので拒否する。
    if payload.user_id != auth.user_id {
        return Err(AppError::Forbidden);
    }
    if payload.tenant_id != tenant_id || payload.project_id != project_id {
        return Err(AppError::BadRequest);
    }

    let app = github_app(&state.http_client, github);
    // インストールがもう無い（アンインストール / 停止）なら選択トークンは死んでいるので
    // 4xx で返す。一時障害まで 4xx にすると、フロントが有効なトークンを捨ててしまう。
    let access = app
        .installation_access_token(payload.installation_id)
        .await
        .map_err(|e| {
            tracing::warn!(
                error = %e,
                installation_id = payload.installation_id,
                "github installation access token failed"
            );
            if is_installation_gone(&e) {
                AppError::BadRequest
            } else {
                AppError::Internal(e)
            }
        })?;
    let repositories = app
        .list_repositories(&access.token)
        .await
        .map_err(AppError::Internal)?;
    Ok((payload, repositories, access))
}

/// 選択トークンを載せるリクエストヘッダー。
///
/// callback はトークンをクエリではなくフラグメントで返している（クエリだと
/// frontend / CDN のアクセスログと Referer に残るため）。ここでクエリに載せ直すと
/// backend とその手前のプロキシのアクセスログに残り、その手当てが台無しになるので、
/// ヘッダーで受け取る。
const SELECT_TOKEN_HEADER: &str = "X-Github-Select-Token";

fn select_token_from_headers(headers: &HeaderMap) -> Result<&str, AppError> {
    headers
        .get(SELECT_TOKEN_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .ok_or(AppError::BadRequest)
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/repositories",
    tag = "GitHub",
    summary = "選択トークンに紐づくリポジトリ一覧",
    params(
        ("tenant_id" = Uuid, Path),
        ("project_id" = Uuid, Path),
        ("X-Github-Select-Token" = String, Header),
    ),
    responses((status = 200, body = GithubRepositoriesResponse), CrudErrors)
)]
pub async fn list_github_repositories(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Json<GithubRepositoriesResponse>, AppError> {
    state.settings.require_github_app()?;
    auth.require_session()?;
    require_tenant_owner(&state, tenant_id, auth.user_id).await?;
    require_project_in_tenant(&state, tenant_id, project_id).await?;

    let select_token = select_token_from_headers(&headers)?;
    // 一覧は何度でも開けるようトークンを消費しない（確定は POST /connect 側）。
    let stored = install_state::peek_select_token(&state.redis_client, select_token)
        .await
        .map_err(AppError::Internal)?;
    let (_, repositories, _) =
        resolve_select_token(&state, &auth, tenant_id, project_id, stored).await?;

    Ok(Json(GithubRepositoriesResponse {
        repositories: repositories
            .into_iter()
            .map(|r| GithubRepositoryItem {
                owner: r.owner,
                name: r.name,
            })
            .collect(),
    }))
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/connect",
    tag = "GitHub",
    summary = "選択したリポジトリを連携",
    params(
        ("tenant_id" = Uuid, Path),
        ("project_id" = Uuid, Path),
    ),
    request_body = GithubConnectRequest,
    responses((status = 204, description = "連携完了"), CrudErrors)
)]
pub async fn connect_github_repository(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<GithubConnectRequest>,
) -> Result<StatusCode, AppError> {
    let github = state.settings.require_github_app()?;
    auth.require_session()?;
    require_tenant_owner(&state, tenant_id, auth.user_id).await?;
    require_project_in_tenant(&state, tenant_id, project_id).await?;

    let stored = install_state::peek_select_token(&state.redis_client, &body.select_token)
        .await
        .map_err(AppError::Internal)?;
    let (payload, repositories, access) =
        resolve_select_token(&state, &auth, tenant_id, project_id, stored).await?;

    // 別タブに残った古い選択トークンで、連携先が黙って巻き戻るのを防ぐ。
    // 連携済みなら、その installation に対するトークンでなければ受け付けない。
    if let Some(current) = github_integrations::Entity::find()
        .filter(github_integrations::Column::ProjectId.eq(project_id))
        .one(&state.db)
        .await?
        && current.installation_id != payload.installation_id
    {
        return Err(AppError::BadRequest);
    }

    // 送られてきたリポジトリが、その installation の可視範囲にあることを必ず確認する。
    if !contains_repository(&repositories, &body.repo_owner, &body.repo_name) {
        return Err(AppError::BadRequest);
    }

    // 検証を通った 1 リクエストだけを DB 更新へ通す（再利用防止）。
    // ここまで消さないので、検証で弾かれたユーザーは選び直せる。
    // 同じトークンでの POST が同時に来ても、取れなかった側はここで止まる。
    let Some((claimed, select_token_ttl)) =
        install_state::claim_select_token(&state.redis_client, &body.select_token)
            .await
            .map_err(AppError::Internal)?
    else {
        return Err(AppError::BadRequest);
    };

    if let Err(e) = upsert_integration(
        &state,
        github,
        project_id,
        auth.user_id,
        claimed.installation_id,
        &body.repo_owner,
        &body.repo_name,
        &access,
    )
    .await
    {
        // 連携できていないので、選び直せるようにトークンを戻す（有効期限は延ばさない）。
        if let Err(restore_err) = install_state::restore_select_token(
            &state.redis_client,
            &body.select_token,
            &claimed,
            select_token_ttl,
        )
        .await
        {
            tracing::warn!(error = %restore_err, "restore github repo select token failed");
        }
        return Err(e);
    }

    // 連携できたので控えを捨てる。
    // 連携自体は成功しているので、後片付けの失敗ではエラーを返さない（TTL で切れる）。
    if let Err(e) = install_state::delete_pending_installation_if(
        &state.redis_client,
        project_id,
        claimed.installation_id,
    )
    .await
    {
        tracing::warn!(error = %e, "discard pending github installation failed; TTL will expire it");
    }

    Ok(StatusCode::NO_CONTENT)
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

    // 同じインストールを他のプロジェクトも使っていたら、GitHub 側は消さない
    // （アンインストールはアカウント単位なので、他のプロジェクトの連携ごと壊れる）。
    let shared = github_integrations::Entity::find()
        .filter(github_integrations::Column::InstallationId.eq(installation_id))
        .filter(github_integrations::Column::ProjectId.ne(project_id))
        .one(&state.db)
        .await?
        .is_some();

    // GitHub 側を先に解除する（404/410 は冪等成功として delete_installation 内で処理済み）。
    // DB を先に削除すると GitHub 側の失敗時に installation_id が失われ再試行不能になる。
    if !shared {
        github_app(&state.http_client, github)
            .delete_installation(installation_id)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, installation_id, "github delete_installation failed");
                AppError::Internal(e)
            })?;
    }

    let active: github_integrations::ActiveModel = row.into();
    active.delete(&state.db).await?;

    // 解除は済んでいるので、後片付けの失敗で失敗扱いにしない（TTL で切れる）。
    if let Err(e) = install_state::delete_pending_installation_if(
        &state.redis_client,
        project_id,
        installation_id,
    )
    .await
    {
        tracing::warn!(error = %e, "discard pending github installation failed; TTL will expire it");
    }

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

    github_issue_sync::enqueue(
        &state.github_issue_sync_storage,
        GithubIssueSyncJob::Import { project_id },
    )
    .await
    .map_err(AppError::Internal)?;

    Ok(StatusCode::ACCEPTED)
}
