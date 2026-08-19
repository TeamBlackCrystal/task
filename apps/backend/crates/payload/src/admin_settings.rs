use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub struct SystemSettingsResponse {
    pub user_registration_enabled: bool,
    pub drive_default_quota_mb: i64,
    pub drive_system_max_quota_mb: i64,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateSystemSettingsRequest {
    pub user_registration_enabled: Option<bool>,
    pub drive_default_quota_mb: Option<i64>,
    pub drive_system_max_quota_mb: Option<i64>,
}
