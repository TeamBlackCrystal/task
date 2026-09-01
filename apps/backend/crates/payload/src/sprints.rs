use chrono::{DateTime, NaiveDate, Utc};
use sea_orm::prelude::Uuid;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use validator::Validate;

use entity::sprints::{self, SprintStatus};

#[derive(Validate, Deserialize, ToSchema)]
pub struct CreateSprintRequest {
    #[validate(length(min = 1, max = 255))]
    pub name: String,
    pub goal: Option<String>,
    #[schema(value_type = String, example = "2026-06-01")]
    pub start_date: NaiveDate,
    #[schema(value_type = String, example = "2026-06-14")]
    pub end_date: NaiveDate,
}

#[derive(Validate, Deserialize, ToSchema)]
pub struct UpdateSprintRequest {
    #[validate(length(min = 1, max = 255))]
    pub name: Option<String>,
    pub goal: Option<String>,
    #[serde(default)]
    pub clear_goal: bool,
    #[schema(value_type = Option<String>, example = "2026-06-01")]
    pub start_date: Option<NaiveDate>,
    #[schema(value_type = Option<String>, example = "2026-06-14")]
    pub end_date: Option<NaiveDate>,
}

#[derive(Deserialize, ToSchema, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListSprintsQuery {
    pub status: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct CompleteSprintRequest {
    #[schema(value_type = Option<String>, format = "uuid")]
    pub move_incomplete_to_sprint_id: Option<Uuid>,
    #[serde(default)]
    pub move_incomplete_to_backlog: bool,
}

#[derive(Validate, Deserialize, ToSchema)]
pub struct AssignTasksRequest {
    #[validate(length(min = 1))]
    pub task_ids: Vec<Uuid>,
}

#[derive(Serialize, ToSchema)]
pub struct SprintTaskCounts {
    pub total: usize,
    pub done: usize,
    pub in_progress: usize,
}

#[derive(Serialize, ToSchema)]
pub struct BurndownPoint {
    #[schema(value_type = String, example = "2026-06-01")]
    pub date: NaiveDate,
    pub ideal_remaining: i32,
    pub actual_remaining: usize,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SprintResponse {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub project_id: Uuid,
    pub name: String,
    #[schema(nullable)]
    pub goal: Option<String>,
    #[schema(value_type = String, example = "2026-06-01")]
    pub start_date: NaiveDate,
    #[schema(value_type = String, example = "2026-06-14")]
    pub end_date: NaiveDate,
    pub status: SprintStatus,
    #[schema(value_type = String, format = "uuid")]
    pub created_by: Uuid,
    #[schema(value_type = String, format = "date-time")]
    pub created_at: DateTime<Utc>,
    #[schema(value_type = String, format = "date-time")]
    pub updated_at: DateTime<Utc>,
}

impl From<sprints::Model> for SprintResponse {
    fn from(model: sprints::Model) -> Self {
        Self {
            id: model.id,
            project_id: model.project_id,
            name: model.name,
            goal: model.goal,
            start_date: model.start_date,
            end_date: model.end_date,
            status: model.status,
            created_by: model.created_by,
            created_at: model.created_at.with_timezone(&Utc),
            updated_at: model.updated_at.with_timezone(&Utc),
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct SprintDetail {
    #[serde(flatten)]
    pub sprint: SprintResponse,
    pub task_counts: SprintTaskCounts,
    pub burndown: Vec<BurndownPoint>,
}
