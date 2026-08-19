use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

#[derive(Serialize, ToSchema)]
pub struct Login2faResponse {
    pub requires_2fa: bool,
    pub requires_2fa_setup: bool,
}

#[derive(Serialize, ToSchema)]
pub struct TotpSetupResponse {
    pub otpauth_uri: String,
    pub qr_code_png: String,
}

#[derive(Validate, Deserialize, ToSchema)]
pub struct TotpCodeRequest {
    #[validate(length(min = 6, max = 8))]
    pub code: String,
}

#[derive(Serialize, ToSchema)]
pub struct VerifySetupResponse {
    pub recovery_codes: Vec<String>,
}

#[derive(Validate, Deserialize, ToSchema)]
pub struct Verify2faRequest {
    #[validate(length(min = 6, max = 20))]
    pub code: Option<String>,
    pub recovery_code: Option<String>,
}

#[derive(Validate, Deserialize, ToSchema)]
pub struct Require2faPolicyRequest {
    pub enabled: bool,
}
