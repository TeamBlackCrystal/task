use serde::Deserialize;
use utoipa::ToSchema;
use validator::Validate;

#[derive(Validate, Debug, Deserialize, ToSchema)]
pub struct LoginRequest {
    #[schema(value_type = String, format="email")]
    #[validate(email)]
    pub email: String,
    #[schema(value_type = String, format="password")]
    #[validate(length(min = 8))]
    pub password: String,
}

#[derive(Validate, Debug, Deserialize, ToSchema)]
pub struct RegisterRequest {
    #[schema(value_type = String, format="username")]
    #[validate(length(min = 3))]
    pub username: String,
    #[schema(value_type = String, format="email")]
    #[validate(email)]
    pub email: String,
    #[schema(value_type = String, format="password")]
    #[validate(length(min = 8))]
    pub password: String,
}

/// メールでの本人確認時に送信する情報。
#[derive(Validate, Debug, Deserialize, ToSchema)]
pub struct VerifyEmailRequest {
    /// メールまたはアプリにお知らせした認証用文字列です。
    #[validate(length(min = 1))]
    pub token: String,
}

#[derive(Validate, Debug, Deserialize, ToSchema)]
pub struct ResendVerificationRequest {
    #[schema(value_type = String, format="email")]
    #[validate(email)]
    pub email: String,
}
