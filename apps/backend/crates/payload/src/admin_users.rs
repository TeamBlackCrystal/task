use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct AdminCreateUserRequest {
    #[validate(length(min = 3))]
    pub username: String,
    #[validate(email)]
    pub email: String,
    #[validate(length(min = 8))]
    pub password: String,
    #[serde(default)]
    pub is_admin: bool,
    #[serde(default)]
    pub email_verified: bool,
}

#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct AdminUpdateUserRequest {
    pub is_admin: Option<bool>,
    pub is_suspended: Option<bool>,
}

#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct AdminPasswordResetRequest {
    #[validate(email)]
    pub send_to: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminPasswordResetResponse {
    pub message: String,
}
