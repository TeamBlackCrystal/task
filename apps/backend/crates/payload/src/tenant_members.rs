use sea_orm::prelude::Uuid;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

use entity::tenant_members::{self, TenantRole};

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TenantMemberResponse {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub tenant_id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub user_id: Uuid,
    pub role: TenantRole,
}

impl From<tenant_members::Model> for TenantMemberResponse {
    fn from(model: tenant_members::Model) -> Self {
        Self {
            id: model.id,
            tenant_id: model.tenant_id,
            user_id: model.user_id,
            role: model.role,
        }
    }
}

#[derive(Validate, Debug, Deserialize, ToSchema)]
pub struct AddTenantMemberRequest {
    #[schema(value_type = String, format = "uuid")]
    pub user_id: Uuid,
    pub role: TenantRole,
}

#[derive(Validate, Debug, Deserialize, ToSchema)]
pub struct UpdateTenantMemberRequest {
    pub role: TenantRole,
}
