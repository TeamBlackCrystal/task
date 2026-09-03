use chrono::{DateTime, Utc};
use sea_orm::prelude::Uuid;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

use entity::{personal_tokens, scopes::Scope, scopes::ScopeList};

#[derive(Validate, Debug, Deserialize, ToSchema)]
pub struct CreatePersonalTokenRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: String,
    #[schema(value_type = String, format = "uuid")]
    pub tenant_id: Uuid,
    #[schema(value_type = Vec<String>, format = "uuid", nullable)]
    pub project_ids: Option<Vec<Uuid>>,
    #[validate(length(min = 1))]
    #[schema(min_items = 1)]
    pub scopes: Vec<Scope>,
    #[schema(value_type = String, format = "date-time", nullable)]
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Validate, Debug, Deserialize, ToSchema)]
pub struct RevokeAllPersonalTokensRequest {
    #[schema(value_type = String, format = "uuid")]
    pub confirm_tenant_id: Uuid,
}

/// PAT のメタデータ（平文トークン・ハッシュは含まない）
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PersonalTokenResponse {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub name: String,
    pub token_last_four: String,
    #[schema(value_type = String, format = "uuid")]
    pub tenant_id: Uuid,
    #[schema(value_type = Vec<String>, format = "uuid", nullable)]
    pub project_ids: Option<Vec<Uuid>>,
    pub scopes: ScopeList,
    #[schema(value_type = String, format = "date-time", nullable)]
    pub expires_at: Option<DateTime<Utc>>,
    #[schema(value_type = String, format = "date-time", nullable)]
    pub last_used_at: Option<DateTime<Utc>>,
    pub revoked: bool,
    #[schema(value_type = String, format = "uuid")]
    pub user_id: Uuid,
}

impl TryFrom<personal_tokens::Model> for PersonalTokenResponse {
    type Error = serde_json::Error;

    fn try_from(model: personal_tokens::Model) -> Result<Self, Self::Error> {
        let project_ids = match model.allowed_project_ids.as_ref() {
            None => None,
            Some(v) => personal_tokens::parse_allowed_project_ids(v)?,
        };

        Ok(Self {
            id: model.id,
            name: model.name,
            token_last_four: model.token_last_four,
            tenant_id: model.tenant_id,
            project_ids,
            expires_at: model.expires_at.map(|dt| dt.with_timezone(&Utc)),
            last_used_at: model.last_used_at.map(|dt| dt.with_timezone(&Utc)),
            revoked: model.revoked,
            user_id: model.user_id,
            scopes: model.scopes,
        })
    }
}

/// PAT 作成時のレスポンス（平文トークンはこの応答でのみ返却）
#[derive(Clone, Serialize, ToSchema)]
pub struct CreatePersonalTokenResponse {
    pub token: String,
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub name: String,
    pub token_last_four: String,
    #[schema(value_type = String, format = "uuid")]
    pub tenant_id: Uuid,
    #[schema(value_type = Vec<String>, format = "uuid", nullable)]
    pub project_ids: Option<Vec<Uuid>>,
    pub scopes: ScopeList,
    #[schema(value_type = String, format = "date-time", nullable)]
    pub expires_at: Option<DateTime<Utc>>,
    #[schema(value_type = String, format = "date-time", nullable)]
    pub last_used_at: Option<DateTime<Utc>>,
    pub revoked: bool,
    #[schema(value_type = String, format = "uuid")]
    pub user_id: Uuid,
}

impl CreatePersonalTokenResponse {
    pub fn new(token: String, model: personal_tokens::Model) -> Result<Self, serde_json::Error> {
        let metadata = PersonalTokenResponse::try_from(model)?;
        Ok(Self {
            token,
            id: metadata.id,
            name: metadata.name,
            token_last_four: metadata.token_last_four,
            tenant_id: metadata.tenant_id,
            project_ids: metadata.project_ids,
            expires_at: metadata.expires_at,
            last_used_at: metadata.last_used_at,
            revoked: metadata.revoked,
            user_id: metadata.user_id,
            scopes: metadata.scopes,
        })
    }
}
