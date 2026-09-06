use sea_orm::prelude::Uuid;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

use crate::users::UserSummary;
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
    /// 表示用のユーザー情報。メンバー管理 UI が名前・アバターを引けるように同梱する
    pub user: UserSummary,
}

impl TenantMemberResponse {
    pub fn from_parts(member: tenant_members::Model, user: entity::users::Model) -> Self {
        Self {
            id: member.id,
            tenant_id: member.tenant_id,
            user_id: member.user_id,
            role: member.role,
            user: user.into(),
        }
    }

    /// テナント owner は `tenant_members` に行を持たないため、一覧表示用に合成する。
    pub fn from_owner(tenant_id: Uuid, user: entity::users::Model) -> Self {
        Self {
            id: user.id,
            tenant_id,
            user_id: user.id,
            role: TenantRole::Admin,
            user: user.into(),
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
