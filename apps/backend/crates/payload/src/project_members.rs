use sea_orm::prelude::Uuid;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

use crate::users::UserSummary;
use entity::project_members::{self, ProjectRole};

#[derive(Debug, Clone, Serialize, ToSchema, serde::Deserialize)]
pub struct ProjectMemberResponse {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub project_id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub user_id: Uuid,
    pub role: ProjectRole,
    /// 表示用のユーザー情報。メンバー管理 UI が名前・アバターを引けるように同梱する
    pub user: UserSummary,
}

impl ProjectMemberResponse {
    pub fn from_parts(member: project_members::Model, user: entity::users::Model) -> Self {
        Self {
            id: member.id,
            project_id: member.project_id,
            user_id: member.user_id,
            role: member.role,
            user: user.into(),
        }
    }
}

#[derive(Validate, Debug, Deserialize, ToSchema)]
pub struct AddMemberRequest {
    #[schema(value_type = String, format = "uuid")]
    pub user_id: Uuid,
    pub role: ProjectRole,
}

#[derive(Validate, Debug, Deserialize, ToSchema)]
pub struct UpdateMemberRequest {
    pub role: ProjectRole,
}
