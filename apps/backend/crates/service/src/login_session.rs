//! 第一認証（パスワード / OAuth コールバック）成功後のセッション確立。
//!
//! ログイン経路ごとに実装を持つと、テナントの 2FA 強制がログイン方法によって
//! 効いたり効かなかったりする（実際に OAuth 経路だけすり抜けていた）。
//! handler 間で共有するのでここに一本化する。

use axum_session::Session;
use axum_session_redispool::SessionRedisPool;
use chrono::Utc;
use sea_orm::prelude::Uuid;
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, JoinType, QueryFilter, QuerySelect, RelationTrait,
};

use entity::{tenant_members, tenants, totp_credentials, users};
use payload::auth_2fa::Login2faResponse;

use crate::auth::AuthError;

async fn user_has_active_2fa(db: &DatabaseConnection, user_id: Uuid) -> Result<bool, AuthError> {
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

/// ユーザーが関わるテナント — テナントオーナー・テナントメンバー・project-only の客分
/// （`project_members` の明示指定だけで関わる者）— のいずれかで
/// `require_2fa=true` が設定されているかを判定する。
/// 2FA セットアップ強制（`user_must_setup_2fa`）と 2FA 無効化禁止（`delete_totp`）の
/// 双方で参照する共通ポリシー判定。
pub async fn user_in_require_2fa_tenant(
    db: &DatabaseConnection,
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
    if member_of_required_tenant {
        return Ok(true);
    }
    // project-only の客分として関わるテナントも対象に含める。残った project_members の
    // 行が実アクセス権（客分）になった以上、ここから漏れると require_2fa テナントの
    // project を 2FA 無しで読み書きできてしまう（docs/features/auth-2fa.md）。
    // 客分テナントの列挙は access::guest_tenant_ids を再利用する（同じ SQL を増やさない）
    let guest_ids = crate::access::guest_tenant_ids(db, user_id)
        .await
        .map_err(|e| AuthError::Internal(anyhow::Error::new(e)))?;
    if guest_ids.is_empty() {
        return Ok(false);
    }
    Ok(tenants::Entity::find()
        .filter(tenants::Column::Id.is_in(guest_ids.into_iter().collect::<Vec<_>>()))
        .filter(tenants::Column::Require2fa.eq(true))
        .one(db)
        .await?
        .is_some())
}

async fn user_must_setup_2fa(db: &DatabaseConnection, user_id: Uuid) -> Result<bool, AuthError> {
    if user_has_active_2fa(db, user_id).await? {
        return Ok(false);
    }
    user_in_require_2fa_tenant(db, user_id).await
}

async fn login_2fa_flags(
    db: &DatabaseConnection,
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
    db: &DatabaseConnection,
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
