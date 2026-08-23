//! 管理者操作の監査ログ記録ヘルパー。

use axum::http::HeaderMap;
use sea_orm::prelude::Uuid;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ConnectionTrait};

use entity::audit_logs;

/// 監査ログを 1 行挿入する。対象操作と同一トランザクションに載せたい場合は
/// `DatabaseTransaction` を渡す（`ConnectionTrait` なのでどちらも受けられる）。
#[allow(clippy::too_many_arguments)]
pub async fn record_audit<C: ConnectionTrait>(
    db: &C,
    actor_id: Uuid,
    action: &str,
    resource_type: &str,
    resource_id: &str,
    tenant_id: Option<Uuid>,
    metadata: Option<serde_json::Value>,
    headers: &HeaderMap,
) -> Result<(), sea_orm::DbErr> {
    let ip_address = headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or(s).trim().to_string());
    let user_agent = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    audit_logs::ActiveModel {
        id: Set(Uuid::new_v4()),
        actor_id: Set(Some(actor_id)),
        actor_type: Set("user".to_string()),
        action: Set(action.to_string()),
        resource_type: Set(resource_type.to_string()),
        resource_id: Set(resource_id.to_string()),
        tenant_id: Set(tenant_id),
        metadata: Set(metadata),
        ip_address: Set(ip_address),
        user_agent: Set(user_agent),
        created_at: Set(chrono::Utc::now().into()),
    }
    .insert(db)
    .await?;

    Ok(())
}
