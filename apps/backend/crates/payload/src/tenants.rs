use sea_orm::prelude::Uuid;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

use entity::tenants;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TenantResponse {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub display_id: String,
    pub name: String,
    pub description: String,
    pub icon_url: String,
    #[schema(value_type = String, format = "uuid")]
    pub owner_id: Uuid,
    #[schema(nullable)]
    pub drive_quota_bytes: Option<i64>,
    pub require_2fa: bool,
}

impl From<tenants::Model> for TenantResponse {
    fn from(model: tenants::Model) -> Self {
        Self {
            id: model.id,
            display_id: model.display_id,
            name: model.name,
            description: model.description,
            icon_url: model.icon_url,
            owner_id: model.owner_id,
            drive_quota_bytes: model.drive_quota_bytes,
            require_2fa: model.require_2fa,
        }
    }
}

#[derive(Validate, Debug, Deserialize, ToSchema)]
pub struct CreateTenantRequest {
    #[validate(length(min = 1))]
    pub display_id: String,
    #[validate(length(min = 1))]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    #[validate(length(max = 700_000))]
    pub icon_url: String,
}

#[derive(Validate, Debug, Deserialize, ToSchema)]
pub struct UpdateTenantRequest {
    #[validate(length(min = 1))]
    pub name: Option<String>,
    pub description: Option<String>,
    /// data URL を受け付ける場合も DB 行と一覧レスポンスを膨らませない上限を設ける。
    #[validate(length(max = 700_000))]
    pub icon_url: Option<String>,
}
