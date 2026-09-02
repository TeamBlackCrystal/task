use chrono::{DateTime, Utc};
use sea_orm::prelude::Uuid;

use entity::drive_folders;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

#[derive(Validate, Debug, Deserialize, ToSchema)]
pub struct CreateFolderRequest {
    #[validate(length(min = 1))]
    pub name: String,
    #[schema(value_type = String, format = "uuid", nullable)]
    pub parent_id: Option<Uuid>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateFolderRequest {
    pub name: Option<String>,
    /// `null` でルートへ移動。フィールド省略時は変更なし。
    #[serde(default, deserialize_with = "crate::nullable::deserialize_some")]
    #[schema(value_type = String, format = "uuid", nullable)]
    pub parent_id: Option<Option<Uuid>>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateShareRequest {
    /// `user` or `public_link`
    #[serde(rename = "type")]
    pub share_type: String,
    #[schema(value_type = String, format = "uuid", nullable)]
    pub user_id: Option<Uuid>,
    pub permission: String,
    #[schema(value_type = String, format = "date-time", nullable)]
    pub expires_at: Option<sea_orm::prelude::DateTimeWithTimeZone>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DriveFolderResponse {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub name: String,
    #[schema(value_type = Option<String>, format = "uuid", nullable)]
    pub parent_id: Option<Uuid>,
    #[schema(value_type = String, format = "uuid")]
    pub tenant_id: Uuid,
    #[schema(value_type = Option<String>, format = "uuid", nullable)]
    pub project_id: Option<Uuid>,
    #[schema(value_type = String, format = "uuid")]
    pub created_by: Uuid,
    #[schema(value_type = String, format = "date-time")]
    pub created_at: DateTime<Utc>,
}

impl From<drive_folders::Model> for DriveFolderResponse {
    fn from(model: drive_folders::Model) -> Self {
        Self {
            id: model.id,
            name: model.name,
            parent_id: model.parent_id,
            tenant_id: model.tenant_id,
            project_id: model.project_id,
            created_by: model.created_by,
            created_at: model.created_at.with_timezone(&Utc),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PublicFolderResponse {
    pub name: String,
    pub created_by_name: String,
    pub file_count: u64,
}
