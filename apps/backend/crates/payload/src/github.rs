use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct GithubCallbackQuery {
    pub installation_id: i64,
    pub state: String,
    /// GitHub が送る操作種別。"request" はオーナー承認待ちであり連携未完了。
    #[serde(default)]
    pub setup_action: Option<String>,
    /// インストール時のユーザー認可で GitHub が付ける認可コード。
    /// これを交換して得たユーザーアクセストークンで、installation の所有者を確認する。
    #[serde(default)]
    pub code: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct GithubIntegrationResponse {
    pub connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String, format = "date-time", nullable)]
    pub connected_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct GithubInstallUrlResponse {
    pub url: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct GithubRepositoryItem {
    pub owner: String,
    pub name: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct GithubRepositoriesResponse {
    pub repositories: Vec<GithubRepositoryItem>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct GithubConnectRequest {
    pub select_token: String,
    pub repo_owner: String,
    pub repo_name: String,
}
