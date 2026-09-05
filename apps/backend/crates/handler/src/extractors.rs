use std::ops::Deref;

use axum::{
    extract::FromRequestParts,
    http::{header::AUTHORIZATION, request::Parts},
};
use axum_session_redispool::SessionRedisPool;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, prelude::Uuid};

use entity::{projects, scopes::Scope, tenants, users};

use std::collections::HashSet;

use crate::auth_helpers::{
    explicit_member_project_ids, is_project_member, is_tenant_member, project_is_open_or_member,
};
use crate::{AppState, error::AppError};
use service::auth::{AuthError, authenticate_personal_token};

type Session = axum_session::Session<SessionRedisPool>;

async fn session_from_parts(parts: &mut Parts, state: &AppState) -> Result<Session, AuthError> {
    Session::from_request_parts(parts, state)
        .await
        .map_err(|_| AuthError::Internal(anyhow::anyhow!("session layer missing")))
}

async fn user_from_session(session: &Session, state: &AppState) -> Result<users::Model, AuthError> {
    let user_id = session
        .get::<Uuid>("user_id")
        .ok_or(AuthError::Unauthorized)?;
    let issued_at_ms = session.get::<i64>("issued_at_ms").unwrap_or(0);

    let user = users::Entity::find_by_id(user_id)
        .one(&state.db)
        .await?
        .ok_or(AuthError::Unauthorized)?;

    if let Some(revoked_at) = user.sessions_revoked_at
        && issued_at_ms < revoked_at.timestamp_millis()
    {
        return Err(AuthError::Unauthorized);
    }

    Ok(user)
}

fn session_is_half_authed(session: &Session) -> bool {
    session.get::<bool>("half_authed").unwrap_or(false)
}

fn bearer_token_from_parts(parts: &Parts) -> Option<String> {
    parts
        .headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|header| header.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(ToOwned::to_owned)
}

#[derive(Debug, Clone)]
pub enum AuthMethod {
    Session,
    PersonalToken {
        token_id: Uuid,
        tenant_id: Uuid,
        allowed_project_ids: Option<Vec<Uuid>>,
        scopes: entity::scopes::ScopeList,
    },
}

/// 認証済みユーザー（セッションまたは PAT）
pub struct AuthUser {
    pub user_id: Uuid,
    pub method: AuthMethod,
}

impl AuthUser {
    /// PAT 管理 API などセッション専用エンドポイント向け。
    pub fn require_session(&self) -> Result<(), AppError> {
        match self.method {
            AuthMethod::Session => Ok(()),
            AuthMethod::PersonalToken { .. } => Err(AppError::Forbidden),
        }
    }

    /// 操作スコープチェック。セッションは常に通過。
    pub fn require_scope(&self, scope: Scope) -> Result<(), AppError> {
        match &self.method {
            AuthMethod::Session => Ok(()),
            AuthMethod::PersonalToken { scopes, .. } => {
                if scopes.has_scope(scope) {
                    Ok(())
                } else {
                    Err(AppError::Forbidden)
                }
            }
        }
    }

    /// テナントオーナー専用操作向け。PAT 境界チェック + オーナー確認を一括実施し、
    /// テナントモデルを返す（呼び出し側で再取得不要）。
    /// `ensure_tenant_access` + `owner_id` 二重チェックの代替として使用する。
    pub async fn ensure_tenant_owner(
        &self,
        state: &AppState,
        tenant_id: Uuid,
    ) -> Result<tenants::Model, AppError> {
        // PAT は自テナント以外への操作を禁止
        if let AuthMethod::PersonalToken {
            tenant_id: pat_tenant,
            allowed_project_ids,
            ..
        } = &self.method
        {
            if tenant_id != *pat_tenant {
                return Err(AppError::Forbidden);
            }
            // プロジェクト制限付き PAT はテナント全体操作不可
            if allowed_project_ids.is_some() {
                return Err(AppError::Forbidden);
            }
        }
        let tenant = tenants::Entity::find_by_id(tenant_id)
            .one(&state.db)
            .await?
            .ok_or(AppError::NotFound)?;
        if tenant.owner_id != self.user_id {
            return Err(AppError::Forbidden);
        }
        Ok(tenant)
    }

    /// テナント / プロジェクト境界チェック。
    pub async fn ensure_tenant_access(
        &self,
        state: &AppState,
        tenant_id: Uuid,
        project_id: Option<Uuid>,
    ) -> Result<(), AppError> {
        if let AuthMethod::PersonalToken {
            tenant_id: pat_tenant,
            allowed_project_ids,
            ..
        } = &self.method
        {
            if tenant_id != *pat_tenant {
                return Err(AppError::Forbidden);
            }
            match project_id {
                Some(project_id) => {
                    if let Some(allowed) = allowed_project_ids
                        && !allowed.contains(&project_id)
                    {
                        return Err(AppError::Forbidden);
                    }
                }
                // プロジェクト制限付き PAT はテナント全体操作（project_id=None）を禁止
                None if allowed_project_ids.is_some() => return Err(AppError::Forbidden),
                None => {}
            }
        }

        // PAT のバインドは「どのテナントを触れるか」の制限であって、所属の証明ではない。
        // テナントから外した利用者のトークンが読み取りを保持しないよう、
        // 所属判定はセッションと同じ経路に通す。
        has_tenant_access(state, self.user_id, tenant_id, project_id).await
    }

    /// テナント一覧系の 2 口（プロジェクト一覧・My Tasks）専用: 通常の所属判定に加えて
    /// project-only の客分も通す。
    ///
    /// 戻り値は客分なら `Some(明示 member の project id 集合)` — 呼び出し側はこれに
    /// 絞って返す。owner / tenant member なら `None` — 従来どおりの規則で返す。
    /// PAT の束縛・scope の層は `ensure_tenant_access` と同じ
    /// （別テナントへの口・project 制限付き PAT のテナント全体操作は従来どおり 403）。
    /// 客分の判別は所属欠落の 403（tenant-membership-missing）だけを拾い、
    /// それ以外の失敗はそのまま返す。
    pub async fn ensure_tenant_access_or_guest_scope(
        &self,
        state: &AppState,
        tenant_id: Uuid,
    ) -> Result<Option<HashSet<Uuid>>, AppError> {
        match self.ensure_tenant_access(state, tenant_id, None).await {
            Ok(()) => Ok(None),
            Err(AppError::ForbiddenDetail(detail)) if detail == "tenant-membership-missing" => {
                let ids = explicit_member_project_ids(&state.db, tenant_id, self.user_id).await?;
                if ids.is_empty() {
                    Err(AppError::ForbiddenDetail(detail))
                } else {
                    Ok(Some(ids))
                }
            }
            Err(e) => Err(e),
        }
    }
}

async fn verify_project_in_tenant(
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

/// テナントに入れるか（＋プロジェクト指定時はその中に入れるか）。
/// セッションと PAT の双方が通る唯一の所属判定。
///
/// project にだけ参加しテナントに参加しない客分（project-only guest）は、
/// 名指しされたプロジェクトの中だけ通る。名指しの無いテナント全体の口は従来どおり 403
/// （apps/backend/docs/tenant-project-authz.md の「所属の 3 層」）。
/// 一覧系 2 口（プロジェクト一覧・My Tasks）の客分への絞り込み開放は
/// `ensure_tenant_access_or_guest_scope` が別途担う。
async fn has_tenant_access(
    state: &AppState,
    user_id: Uuid,
    tenant_id: Uuid,
    project_id: Option<Uuid>,
) -> Result<(), AppError> {
    let tenant = tenants::Entity::find_by_id(tenant_id)
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    if tenant.owner_id == user_id {
        if let Some(pid) = project_id {
            verify_project_in_tenant(state, tenant_id, pid).await?;
        }
        return Ok(());
    }

    // テナントに入れるのはメンバーだけ。プロジェクト単位の絞り込みはその内側で行う。
    // この 403 だけ理由を名指しする: オーナーでなくメンバー行も無い状態は、データ側の
    // 不整合 (owner_id の付け替え・tenant_members の欠落) でも起きるため、一般の 403
    // (プロジェクト非公開・スコープ不足) と見分けられないと「PAT が全 API で 403」の
    // 原因へ利用者が辿り着けない。権限は一切広げない — status は 403 のまま、body の
    // message だけを tenant-membership-missing にする
    if !is_tenant_member(&state.db, tenant_id, user_id).await? {
        // project-only の客分: テナントに行が無くても、名指しされたプロジェクトの
        // `project_members` に明示指定があればそのプロジェクトの中だけ通す。
        // 「メンバー未指定＝テナント全体に開放」の規則はテナントメンバー限りなので
        // 公開規則（`project_is_open_or_member`）ではなく明示指定だけを見る。
        // 存在探りを許さないため、明示指定の確認をプロジェクト実在確認より先に行う
        // （無関係な利用者への応答は従来どおり 403 のまま変わらない）
        if let Some(pid) = project_id
            && is_project_member(&state.db, pid, user_id).await?
        {
            verify_project_in_tenant(state, tenant_id, pid).await?;
            return Ok(());
        }
        return Err(AppError::ForbiddenDetail(
            "tenant-membership-missing".into(),
        ));
    }

    let Some(pid) = project_id else {
        return Ok(());
    };
    verify_project_in_tenant(state, tenant_id, pid).await?;

    if project_is_open_or_member(&state.db, pid, user_id).await? {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

/// 認証任意（未認証は `None`）。コンテンツ配信などで使用。
pub struct OptionalAuthUser(pub Option<AuthUser>);

impl FromRequestParts<AppState> for OptionalAuthUser {
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        match AuthUser::from_request_parts(parts, state).await {
            Ok(auth) => Ok(OptionalAuthUser(Some(auth))),
            // 半認証セッションは未認証扱い（Drive 等の Optional エンドポイントで 403 にしない）
            Err(AuthError::Unauthorized) | Err(AuthError::Forbidden) => Ok(OptionalAuthUser(None)),
            Err(e) => Err(e),
        }
    }
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if let Some(token) = bearer_token_from_parts(parts) {
            let record = authenticate_personal_token(
                &state.db,
                &state.settings.personal_token_secret,
                &token,
            )
            .await?;
            let user = users::Entity::find_by_id(record.user_id)
                .one(&state.db)
                .await?
                .ok_or(AuthError::Unauthorized)?;
            if user.is_suspended {
                return Err(AuthError::Suspended);
            }
            Ok(AuthUser {
                user_id: record.user_id,
                method: AuthMethod::PersonalToken {
                    token_id: record.id,
                    tenant_id: record.tenant_id,
                    allowed_project_ids: match record.allowed_project_ids.as_ref() {
                        None => None,
                        Some(v) => {
                            entity::personal_tokens::parse_allowed_project_ids(v).map_err(|e| {
                                AuthError::Internal(anyhow::anyhow!(
                                    "allowed_project_ids parse error: {e}"
                                ))
                            })?
                        }
                    },
                    scopes: record.scopes.clone(),
                },
            })
        } else {
            let session = session_from_parts(parts, state).await?;
            if session_is_half_authed(&session) {
                return Err(AuthError::Forbidden);
            }
            let user = user_from_session(&session, state).await?;
            if user.is_suspended {
                return Err(AuthError::Suspended);
            }
            Ok(AuthUser {
                user_id: user.id,
                method: AuthMethod::Session,
            })
        }
    }
}

/// 半認証セッション専用（`POST /v1/auth/2fa/verify`）
pub struct HalfAuthedUser {
    pub user_id: Uuid,
}

impl FromRequestParts<AppState> for HalfAuthedUser {
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if bearer_token_from_parts(parts).is_some() {
            return Err(AuthError::Forbidden);
        }
        let session = session_from_parts(parts, state).await?;
        if !session_is_half_authed(&session) {
            return Err(AuthError::Forbidden);
        }
        let user_id = session
            .get::<Uuid>("user_id")
            .ok_or(AuthError::Unauthorized)?;
        let issued_at_ms = session.get::<i64>("issued_at_ms").unwrap_or(0);
        let user = users::Entity::find_by_id(user_id)
            .one(&state.db)
            .await?
            .ok_or(AuthError::Unauthorized)?;
        // Suspend before session-revocation so admin suspend still returns 403 on 2FA verify.
        if user.is_suspended {
            return Err(AuthError::Suspended);
        }
        if let Some(revoked_at) = user.sessions_revoked_at
            && issued_at_ms < revoked_at.timestamp_millis()
        {
            return Err(AuthError::Unauthorized);
        }
        Ok(HalfAuthedUser { user_id: user.id })
    }
}

/// ログイン済みセッション（半認証・完全認証どちらも可）。2FA セットアップ用。
pub struct LoggedInUser {
    pub user_id: Uuid,
}

impl FromRequestParts<AppState> for LoggedInUser {
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if bearer_token_from_parts(parts).is_some() {
            return Err(AuthError::Forbidden);
        }
        let session = session_from_parts(parts, state).await?;
        let user = user_from_session(&session, state).await?;
        if user.is_suspended {
            return Err(AuthError::Suspended);
        }
        Ok(LoggedInUser { user_id: user.id })
    }
}

/// 管理者専用エクストラクタ（セッション認証のみ）
pub struct AdminUser {
    pub user_id: Uuid,
}

impl FromRequestParts<AppState> for AdminUser {
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // PAT ではこのエンドポイント群を認証できない。従来もセッション不在で 401 を返して
        // いたため、拒否のステータスは変えない。
        if bearer_token_from_parts(parts).is_some() {
            return Err(AuthError::Unauthorized);
        }
        let session = session_from_parts(parts, state).await?;
        if session_is_half_authed(&session) {
            return Err(AuthError::Forbidden);
        }
        let user = user_from_session(&session, state).await?;
        if user.is_suspended {
            return Err(AuthError::Suspended);
        }
        if !user.is_admin {
            return Err(AuthError::Forbidden);
        }
        Ok(AdminUser { user_id: user.id })
    }
}

/// 認証済みユーザーの DB レコード（ハンドラで毎回取得する必要なし）
pub struct CurrentUser(pub users::Model);

impl Deref for CurrentUser {
    type Target = users::Model;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // PAT ではこのエンドポイント群を認証できない。従来もセッション不在で 401 を返して
        // いたため、拒否のステータスは変えない。
        if bearer_token_from_parts(parts).is_some() {
            return Err(AuthError::Unauthorized);
        }
        let session = session_from_parts(parts, state).await?;
        if session_is_half_authed(&session) {
            return Err(AuthError::Forbidden);
        }
        let user = user_from_session(&session, state).await?;
        if user.is_suspended {
            return Err(AuthError::Suspended);
        }
        Ok(CurrentUser(user))
    }
}
