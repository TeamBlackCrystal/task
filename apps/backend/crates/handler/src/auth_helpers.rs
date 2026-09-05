use sea_orm::prelude::Uuid;

use crate::AppState;
use crate::error::AppError;

// 実装は service 側に一本化（レビュー指摘: 同一実装の重複解消）。
pub use service::access::{
    guest_tenant_ids, is_project_member, is_tenant_member, project_is_open_or_member,
    visible_project_ids,
};
pub use service::drive::is_tenant_owner;

/// **指定した利用者**がそのプロジェクトに入れるかを確認する。
///
/// リクエスト元自身の認可は `AuthUser::ensure_tenant_access` が同じ判定を含んでいるので、
/// ここを重ねて呼ぶ必要はない。担当者の追加など、自分以外を検証するときだけ使う。
pub async fn require_project_access(
    state: &AppState,
    tenant_id: Uuid,
    project_id: Uuid,
    user_id: Uuid,
) -> Result<(), AppError> {
    if is_tenant_owner(&state.db, tenant_id, user_id).await? {
        return Ok(());
    }
    if !is_tenant_member(&state.db, tenant_id, user_id).await? {
        return Err(AppError::Forbidden);
    }
    if project_is_open_or_member(&state.db, project_id, user_id).await? {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}
