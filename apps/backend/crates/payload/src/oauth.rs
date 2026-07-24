use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Debug, Deserialize)]
pub struct OAuthStartQuery {
    #[serde(default)]
    pub redirect_after: Option<String>,
    /// プロバイダーエラー時の戻り先（OAuth ボタンのあるページ）。未指定なら redirect_after にフォールバック。
    #[serde(default)]
    pub error_redirect_after: Option<String>,
    #[serde(default)]
    pub instance_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OAuthCallbackQuery {
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DisconnectQuery {
    #[serde(default)]
    pub instance_url: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct OAuthConnectionItem {
    pub provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_url: Option<String>,
    pub connected_at: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct OAuthConnectionsResponse {
    pub connections: Vec<OAuthConnectionItem>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct OAuthProviderItem {
    /// プロバイダー slug（github | gitlab | gitlab_selfhosted | google | oidc）
    pub provider: String,
    /// ログイン開始時に self-hosted インスタンス URL の入力が必要か（gitlab_selfhosted のみ true）
    pub requires_instance_url: bool,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct OAuthProvidersResponse {
    pub providers: Vec<OAuthProviderItem>,
}

#[derive(Validate, Debug, Deserialize, utoipa::ToSchema)]
pub struct SetPasswordRequest {
    #[schema(value_type = String, format = "password")]
    #[validate(length(min = 8))]
    pub password: String,
}
