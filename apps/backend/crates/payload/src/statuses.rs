use chrono::{DateTime, Utc};
use sea_orm::prelude::Uuid;
use serde::Deserialize;
use utoipa::ToSchema;
use validator::Validate;

use entity::project_statuses;

#[derive(Debug, Clone, serde::Serialize, ToSchema)]
pub struct ProjectStatusResponse {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub project_id: Uuid,
    pub name: String,
    pub color: String,
    pub position: i16,
    pub is_default: bool,
    pub is_done_state: bool,
    #[schema(value_type = String, format = "date-time")]
    pub created_at: DateTime<Utc>,
}

impl From<project_statuses::Model> for ProjectStatusResponse {
    fn from(model: project_statuses::Model) -> Self {
        Self {
            id: model.id,
            project_id: model.project_id,
            name: model.name,
            color: model.color,
            position: model.position,
            is_default: model.is_default,
            is_done_state: model.is_done_state,
            created_at: model.created_at.with_timezone(&Utc),
        }
    }
}

#[derive(Validate, Deserialize, ToSchema)]
pub struct CreateStatusRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: String,
    #[validate(regex(path = "common::validation::COLOR_REGEX"))]
    pub color: String,
    pub position: i16,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub is_done_state: bool,
}

#[derive(Validate, Deserialize, ToSchema)]
pub struct UpdateStatusRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: Option<String>,
    #[validate(regex(path = "common::validation::COLOR_REGEX"))]
    pub color: Option<String>,
    pub position: Option<i16>,
    pub is_default: Option<bool>,
    pub is_done_state: Option<bool>,
}

#[derive(Deserialize, ToSchema)]
pub struct ReorderRequest {
    pub ids: Vec<Uuid>,
}

#[derive(Deserialize, ToSchema)]
pub struct DeleteStatusQuery {
    pub migrate_to_status_id: Option<Uuid>,
}
