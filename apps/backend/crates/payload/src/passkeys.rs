use sea_orm::prelude::Uuid;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

#[derive(Serialize, ToSchema)]
pub struct PasskeyListItem {
    pub id: Uuid,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<String>,
    pub created_at: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct PasskeyListResponse {
    pub passkeys: Vec<PasskeyListItem>,
}

#[derive(Validate, Deserialize, utoipa::ToSchema)]
pub struct PasskeyRegistrationFinishRequest {
    #[validate(length(min = 1, max = 255))]
    pub name: String,
    #[schema(value_type = Object)]
    pub credential: serde_json::Value,
}

#[derive(Validate, Deserialize, utoipa::ToSchema)]
pub struct PasskeyAuthenticationStartRequest {
    #[validate(email)]
    pub email: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct PasskeyAuthenticationFinishRequest {
    pub challenge_id: Uuid,
    #[schema(value_type = Object)]
    pub credential: serde_json::Value,
}

#[derive(Validate, Deserialize, utoipa::ToSchema)]
pub struct PasskeyRenameRequest {
    #[validate(length(min = 1, max = 255))]
    pub name: String,
}
