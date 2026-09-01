use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

#[derive(Serialize, ToSchema)]
pub struct MessageResponse {
    pub message: String,
}

#[derive(Validate, Debug, Deserialize, ToSchema)]
pub struct PasswordResetRequestBody {
    #[validate(email)]
    pub email: String,
}

#[derive(Validate, Deserialize, ToSchema)]
pub struct PasswordResetVerifyBody {
    #[validate(length(min = 1))]
    pub token: String,
}

#[derive(Validate, Deserialize, ToSchema)]
pub struct PasswordResetCompleteBody {
    #[validate(length(min = 1))]
    pub token: String,
    #[validate(length(min = 8))]
    pub new_password: String,
}

#[derive(Validate, Deserialize, ToSchema)]
pub struct PasswordChangeBody {
    pub current_password: String,
    #[validate(length(min = 8))]
    pub new_password: String,
}

impl std::fmt::Debug for PasswordResetVerifyBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PasswordResetVerifyBody")
            .field("token", &"<redacted>")
            .finish()
    }
}

impl std::fmt::Debug for PasswordResetCompleteBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PasswordResetCompleteBody")
            .field("token", &"<redacted>")
            .field("new_password", &"<redacted>")
            .finish()
    }
}

impl std::fmt::Debug for PasswordChangeBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PasswordChangeBody")
            .field("current_password", &"<redacted>")
            .field("new_password", &"<redacted>")
            .finish()
    }
}
