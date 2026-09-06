use chrono::{DateTime, NaiveDate, Utc};
use sea_orm::prelude::Uuid;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

use entity::milestones;

#[derive(Validate, Deserialize, ToSchema)]
pub struct CreateMilestoneRequest {
    #[validate(length(min = 1, max = 255))]
    pub name: String,
    pub description: Option<String>,
    pub due_date: NaiveDate,
}

#[derive(Validate, Deserialize, ToSchema)]
pub struct UpdateMilestoneRequest {
    #[validate(length(min = 1, max = 255))]
    pub name: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub clear_description: bool,
    pub due_date: Option<NaiveDate>,
}

#[derive(Debug, Clone, Serialize, ToSchema, serde::Deserialize)]
pub struct MilestoneResponse {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub project_id: Uuid,
    pub name: String,
    #[schema(nullable)]
    pub description: Option<String>,
    #[schema(value_type = String, example = "2026-07-01")]
    pub due_date: NaiveDate,
    #[schema(value_type = String, format = "uuid")]
    pub created_by: Uuid,
    #[schema(value_type = String, format = "date-time")]
    pub created_at: DateTime<Utc>,
    #[schema(value_type = String, format = "date-time")]
    pub updated_at: DateTime<Utc>,
}

impl From<milestones::Model> for MilestoneResponse {
    fn from(model: milestones::Model) -> Self {
        Self {
            id: model.id,
            project_id: model.project_id,
            name: model.name,
            description: model.description,
            due_date: model.due_date,
            created_by: model.created_by,
            created_at: model.created_at.with_timezone(&Utc),
            updated_at: model.updated_at.with_timezone(&Utc),
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct MilestoneDetail {
    #[serde(flatten)]
    pub milestone: MilestoneResponse,
    pub progress_pct: u32,
    pub task_counts: TaskCounts,
}

#[derive(Serialize, ToSchema)]
pub struct TaskCounts {
    pub total: usize,
    pub done: usize,
}
