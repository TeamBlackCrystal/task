use chrono::{DateTime, Utc};
use sea_orm::prelude::Uuid;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use entity::audit_logs;

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AuditLogQuery {
    /// アクション名の前方一致フィルタ
    pub action: Option<String>,
    pub resource_type: Option<String>,
    pub resource_id: Option<String>,
    pub actor_id: Option<Uuid>,
    pub tenant_id: Option<Uuid>,
    /// 期間開始（ISO 8601）
    pub from: Option<String>,
    /// 期間終了（ISO 8601）
    pub to: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: u64,
    pub cursor: Option<String>,
}

fn default_limit() -> u64 {
    50
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AuditLogResponse {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    #[schema(value_type = Option<String>, format = "uuid", nullable)]
    pub actor_id: Option<Uuid>,
    pub actor_type: String,
    pub action: String,
    pub resource_type: String,
    pub resource_id: String,
    #[schema(value_type = Option<String>, format = "uuid", nullable)]
    pub tenant_id: Option<Uuid>,
    pub metadata: Option<serde_json::Value>,
    #[schema(nullable)]
    pub ip_address: Option<String>,
    #[schema(nullable)]
    pub user_agent: Option<String>,
    #[schema(value_type = String, format = "date-time")]
    pub created_at: DateTime<Utc>,
}

impl From<audit_logs::Model> for AuditLogResponse {
    fn from(model: audit_logs::Model) -> Self {
        Self {
            id: model.id,
            actor_id: model.actor_id,
            actor_type: model.actor_type,
            action: model.action,
            resource_type: model.resource_type,
            resource_id: model.resource_id,
            tenant_id: model.tenant_id,
            metadata: model.metadata,
            ip_address: model.ip_address,
            user_agent: model.user_agent,
            created_at: model.created_at.with_timezone(&Utc),
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct AuditLogListResponse {
    pub logs: Vec<AuditLogResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}
