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
    pub icon_url: String,
}

#[derive(Validate, Debug, Deserialize, ToSchema)]
pub struct UpdateTenantRequest {
    #[validate(length(min = 1))]
    pub name: Option<String>,
    pub description: Option<String>,
    pub icon_url: Option<String>,
}

/// テナント一覧における、この利用者から見た関わり方の印。
/// `TenantRole` と同じ流儀（PascalCase の文字列）で返す。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
pub enum TenantMembershipKind {
    /// `tenants.owner_id` の本人
    Owner,
    /// `tenant_members` に行がある（ロールの内訳は `TenantRole` が別途表す）
    Member,
    /// project-only の客分（`project_members` の明示指定だけで関わる）
    Guest,
}

/// `GET /v1/tenants` 専用のレスポンス。テナントの欄に `membership` の印を加えたもの。
///
/// 客分（membership=Guest）のテナントは一覧に出るが tenant-wide の口
/// （テナント取得・プロジェクト一覧など）は開かないため、クライアントは
/// この印で開ける口を見分ける。取得・作成・更新は従来どおり `TenantResponse`。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TenantListItemResponse {
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
    /// この利用者から見た関わり方（Owner / Member / Guest）
    pub membership: TenantMembershipKind,
}

impl TenantListItemResponse {
    pub fn from_parts(model: tenants::Model, membership: TenantMembershipKind) -> Self {
        Self {
            id: model.id,
            display_id: model.display_id,
            name: model.name,
            description: model.description,
            icon_url: model.icon_url,
            owner_id: model.owner_id,
            drive_quota_bytes: model.drive_quota_bytes,
            require_2fa: model.require_2fa,
            membership,
        }
    }
}
