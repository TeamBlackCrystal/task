use chrono::{DateTime, Utc};
use sea_orm::prelude::Uuid;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

use crate::custom_fields::{CustomFieldValueInput, TaskCustomFieldValueResponse};
use crate::labels::LabelResponse;
use crate::users::UserSummary;
use entity::{task_assignees, task_relations, tasks};

#[derive(Debug, Clone, Serialize, ToSchema, serde::Deserialize)]
pub struct TaskResponse {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub project_id: Uuid,
    pub seq_id: i32,
    pub title: String,
    #[schema(nullable)]
    pub description: Option<String>,
    #[schema(value_type = String, format = "uuid")]
    pub status_id: Uuid,
    pub priority: tasks::TaskPriority,
    pub progress_pct: i16,
    #[schema(value_type = Option<String>, format = "uuid", nullable)]
    pub parent_task_id: Option<Uuid>,
    #[schema(value_type = Option<String>, format = "uuid", nullable)]
    pub milestone_id: Option<Uuid>,
    #[schema(value_type = Option<String>, format = "uuid", nullable)]
    pub sprint_id: Option<Uuid>,
    #[schema(value_type = Option<String>, format = "date-time", nullable)]
    pub soft_deadline: Option<DateTime<Utc>>,
    #[schema(value_type = Option<String>, format = "date-time", nullable)]
    pub hard_deadline: Option<DateTime<Utc>>,
    #[schema(nullable)]
    pub estimated_minutes: Option<i32>,
    pub is_archived: bool,
    /// 作成者。ユーザー行が欠損している場合（FK 制約外の経路で削除された等）は null
    #[schema(nullable)]
    pub created_by: Option<UserSummary>,
    pub assignees: Vec<TaskAssigneeSummary>,
    pub labels: Vec<LabelResponse>,
    #[schema(value_type = String, format = "date-time")]
    pub created_at: DateTime<Utc>,
    #[schema(value_type = String, format = "date-time")]
    pub updated_at: DateTime<Utc>,
    #[schema(value_type = Option<String>, format = "date-time", nullable)]
    pub completed_at: Option<DateTime<Utc>>,
    #[schema(value_type = Option<String>, format = "date-time", nullable)]
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, ToSchema, serde::Deserialize)]
pub struct TaskAssigneeSummary {
    pub role: String,
    pub user: UserSummary,
}

impl TaskResponse {
    pub fn from_parts(
        model: tasks::Model,
        created_by: Option<UserSummary>,
        assignees: Vec<TaskAssigneeSummary>,
        labels: Vec<LabelResponse>,
    ) -> Self {
        Self {
            id: model.id,
            project_id: model.project_id,
            seq_id: model.seq_id,
            title: model.title,
            description: model.description,
            status_id: model.status_id,
            priority: model.priority,
            progress_pct: model.progress_pct,
            parent_task_id: model.parent_task_id,
            milestone_id: model.milestone_id,
            sprint_id: model.sprint_id,
            soft_deadline: model.soft_deadline.map(|dt| dt.with_timezone(&Utc)),
            hard_deadline: model.hard_deadline.map(|dt| dt.with_timezone(&Utc)),
            estimated_minutes: model.estimated_minutes,
            is_archived: model.is_archived,
            created_by,
            assignees,
            labels,
            created_at: model.created_at.with_timezone(&Utc),
            updated_at: model.updated_at.with_timezone(&Utc),
            completed_at: model.completed_at.map(|dt| dt.with_timezone(&Utc)),
            deleted_at: model.deleted_at.map(|dt| dt.with_timezone(&Utc)),
        }
    }
}

#[derive(Deserialize, ToSchema, serde::Serialize)]
pub struct AssigneeInput {
    pub user_id: Uuid,
    pub role: String,
}

#[derive(Validate, Deserialize, ToSchema, serde::Serialize)]
pub struct CreateTaskRequest {
    #[validate(length(min = 1, max = 255))]
    pub title: String,
    pub description: Option<String>,
    #[schema(value_type = String, format = "uuid")]
    pub status_id: Uuid,
    pub priority: Option<tasks::TaskPriority>,
    #[validate(range(min = 0, max = 100))]
    pub progress_pct: Option<i16>,
    #[schema(value_type = Option<String>, format = "uuid")]
    pub parent_task_id: Option<Uuid>,
    #[schema(value_type = Option<String>, format = "uuid")]
    pub milestone_id: Option<Uuid>,
    #[schema(value_type = Option<String>, format = "uuid")]
    pub sprint_id: Option<Uuid>,
    #[schema(value_type = Option<String>, format = "date-time")]
    pub soft_deadline: Option<chrono::DateTime<chrono::Utc>>,
    #[schema(value_type = Option<String>, format = "date-time")]
    pub hard_deadline: Option<chrono::DateTime<chrono::Utc>>,
    #[validate(range(min = 1))]
    pub estimated_minutes: Option<i32>,
    #[serde(default)]
    pub assignees: Vec<AssigneeInput>,
    #[serde(default)]
    pub label_ids: Vec<Uuid>,
    #[serde(default)]
    pub custom_field_values: Vec<CustomFieldValueInput>,
}

#[derive(Validate, Deserialize, ToSchema, serde::Serialize)]
pub struct UpdateTaskRequest {
    #[validate(length(min = 1, max = 255))]
    pub title: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub clear_description: bool,
    #[schema(value_type = Option<String>, format = "uuid")]
    pub status_id: Option<Uuid>,
    pub priority: Option<tasks::TaskPriority>,
    #[validate(range(min = 0, max = 100))]
    pub progress_pct: Option<i16>,
    #[schema(value_type = Option<String>, format = "uuid")]
    pub parent_task_id: Option<Uuid>,
    #[serde(default)]
    pub clear_parent_task_id: bool,
    #[schema(value_type = Option<String>, format = "uuid")]
    pub milestone_id: Option<Uuid>,
    #[serde(default)]
    pub clear_milestone_id: bool,
    #[schema(value_type = Option<String>, format = "uuid")]
    pub sprint_id: Option<Uuid>,
    #[serde(default)]
    pub clear_sprint_id: bool,
    #[schema(value_type = Option<String>, format = "date-time")]
    pub soft_deadline: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub clear_soft_deadline: bool,
    #[schema(value_type = Option<String>, format = "date-time")]
    pub hard_deadline: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub clear_hard_deadline: bool,
    #[validate(range(min = 1))]
    pub estimated_minutes: Option<i32>,
    #[serde(default)]
    pub clear_estimated_minutes: bool,
    pub is_archived: Option<bool>,
    /// タスクのラベルをこの ID 集合で置き換える（`Some(vec![])` で全解除）。None は変更なし
    pub label_ids: Option<Vec<Uuid>>,
    pub custom_field_values: Option<Vec<CustomFieldValueInput>>,
}

#[derive(Deserialize, ToSchema, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListTasksQuery {
    pub status_id: Option<Uuid>,
    pub priority: Option<String>,
    pub assignee_id: Option<Uuid>,
    pub label_id: Option<Uuid>,
    pub milestone_id: Option<Uuid>,
    pub sprint_id: Option<Uuid>,
    pub parent_task_id: Option<Uuid>,
    #[serde(default)]
    pub is_archived: bool,
    pub sort: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: u64,
    /// ページ番号で飛ぶ一覧（表形式）が使う。続きを読み足す用途には `cursor` を使う
    #[serde(default)]
    pub offset: u64,
    /// 前のページの `next_cursor`。先頭ページでは付けない。
    ///
    /// `offset` と同時には指定できない（400）。取得のあいだにタスクが増減すると
    /// `offset` の境界がずれ、境界の行がどちらのページにも出なくなるため、
    /// 続きを読み足す一覧はこちらを使う（`common::cursor`）。
    /// カーソルは `sort` ごとに別物なので、並び替えを変えたら先頭から取り直す
    pub cursor: Option<String>,
}

fn default_limit() -> u64 {
    50
}

#[derive(Serialize, ToSchema, serde::Deserialize)]
pub struct TaskListResponse {
    pub tasks: Vec<TaskResponse>,
    /// 絞り込み後の総数。件数の表示とページ番号の算出に使う。
    /// **「まだ残っているか」の判断には使わない**（取得中に動くので終わらなくなる）
    pub total: u64,
    /// 次のページを引く鍵。`null` なら取り切っている。
    /// `cursor` を渡していない要求でも返すので、先頭ページから継いでいける
    #[schema(required, nullable)]
    pub next_cursor: Option<String>,
}

#[derive(Serialize, ToSchema, serde::Deserialize)]
pub struct TaskDetailResponse {
    #[serde(flatten)]
    pub task: TaskResponse,
    pub custom_field_values: Vec<TaskCustomFieldValueResponse>,
}

#[derive(Validate, Deserialize, ToSchema, serde::Serialize)]
pub struct AddAssigneeRequest {
    pub user_id: Uuid,
    #[validate(length(min = 1))]
    pub role: String,
}

#[derive(Validate, Deserialize, ToSchema)]
pub struct UpdateAssigneeRequest {
    #[validate(length(min = 1))]
    pub role: String,
}

#[derive(Debug, Clone, Serialize, ToSchema, serde::Deserialize)]
pub struct TaskAssigneeResponse {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub task_id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub user_id: Uuid,
    pub role: String,
    #[schema(value_type = String, format = "date-time")]
    pub assigned_at: DateTime<Utc>,
}

impl From<task_assignees::Model> for TaskAssigneeResponse {
    fn from(model: task_assignees::Model) -> Self {
        Self {
            id: model.id,
            task_id: model.task_id,
            user_id: model.user_id,
            role: model.role,
            assigned_at: model.assigned_at.with_timezone(&Utc),
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TaskRelationResponse {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub blocker_task_id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub blocked_task_id: Uuid,
    #[schema(value_type = String, format = "date-time")]
    pub created_at: DateTime<Utc>,
}

impl From<task_relations::Model> for TaskRelationResponse {
    fn from(model: task_relations::Model) -> Self {
        Self {
            id: model.id,
            blocker_task_id: model.blocker_task_id,
            blocked_task_id: model.blocked_task_id,
            created_at: model.created_at.with_timezone(&Utc),
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct RelationEntry {
    pub relation_id: Uuid,
    #[serde(flatten)]
    pub task: TaskResponse,
}

#[derive(Serialize, ToSchema)]
pub struct TaskRelationsResponse {
    pub subtasks: Vec<TaskResponse>,
    pub blocks: Vec<RelationEntry>,
    pub blocked_by: Vec<RelationEntry>,
}

#[derive(Deserialize, ToSchema)]
pub struct AddRelationRequest {
    #[serde(rename = "type")]
    pub relation_type: String,
    pub target_task_id: Uuid,
}
