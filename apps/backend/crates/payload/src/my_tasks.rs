use sea_orm::prelude::Uuid;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use entity::tasks;

#[derive(Debug, Deserialize, ToSchema, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListMyTasksQuery {
    #[serde(default = "default_filter")]
    pub filter: String,
    #[serde(default = "default_include_personal")]
    pub include_personal: bool,
    pub project_id: Option<Uuid>,
    #[serde(default = "default_limit")]
    pub limit: u64,
    #[serde(default)]
    pub offset: u64,
}

fn default_filter() -> String {
    "all".to_string()
}

fn default_include_personal() -> bool {
    true
}

fn default_limit() -> u64 {
    50
}

#[derive(Serialize, ToSchema)]
pub struct MyTaskProjectInfo {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub name: String,
    pub key: String,
    pub is_personal: bool,
}

#[derive(Serialize, ToSchema)]
pub struct MyTaskStatusInfo {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub name: String,
    pub color: String,
}

#[derive(Serialize, ToSchema)]
pub struct MyTaskItem {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub seq_id: i32,
    pub seq_key: String,
    pub title: String,
    pub status: MyTaskStatusInfo,
    pub priority: tasks::TaskPriority,
    #[schema(value_type = Option<String>, format = "date-time", nullable)]
    pub soft_deadline: Option<chrono::DateTime<chrono::Utc>>,
    #[schema(value_type = Option<String>, format = "date-time", nullable)]
    pub hard_deadline: Option<chrono::DateTime<chrono::Utc>>,
    pub project: MyTaskProjectInfo,
    pub is_personal: bool,
}

#[derive(Serialize, ToSchema)]
pub struct MyTasksListResponse {
    pub tasks: Vec<MyTaskItem>,
    pub total: u64,
}
