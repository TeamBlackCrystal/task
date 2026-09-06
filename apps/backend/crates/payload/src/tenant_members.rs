use sea_orm::prelude::Uuid;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

use crate::users::UserSummary;
use entity::project_members::ProjectRole;
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

/// その人が持つ、プロジェクト単位の明示 ACE。
/// テナント所属（継承）を外しても明示 ACE は独立して残り、持ち主は客分としてそのプロジェクトに入り続ける。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ExplicitProjectAce {
    #[schema(value_type = String, format = "uuid")]
    pub project_id: Uuid,
    pub key: String,
    pub name: String,
    pub role: ProjectRole,
}

/// その人がこのテナントで持つ明示 ACE の一覧。
/// テナント除名の前に「除名しても、この人はこれらのプロジェクトに入り続ける」と確かめるために使う。
/// 管理者が「除名したのにまだ入れる」と驚かないためのもので、除名の後に呼んでも同じ内容を返す。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ExplicitProjectsResponse {
    /// 呼び出し側から見えるプロジェクトの明示 ACE。
    /// 閉め出したい場合は、ここに挙がったプロジェクトのメンバーからも外す。
    pub explicit_projects: Vec<ExplicitProjectAce>,
    /// 呼び出し側には見えないプロジェクトに残る明示 ACE の件数。
    /// 非公開プロジェクトの名前や key を、そのプロジェクトに入れない人へ出さないために数だけ返す。
    pub hidden_count: u64,
}
