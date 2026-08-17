use sea_orm::prelude::Uuid;

use crate::AppState;
use crate::error::AppError;

// 実装は service 側に一本化（レビュー指摘: 同一実装の重複解消）。
pub use service::access::{is_tenant_member, project_is_open_or_member, visible_project_ids};
pub use service::drive::is_tenant_owner;

pub async fn require_member_or_owner(
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
