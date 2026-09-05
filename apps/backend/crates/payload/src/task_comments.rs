use chrono::{DateTime, Utc};
use sea_orm::prelude::Uuid;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

use entity::task_comments;

#[derive(Debug, Clone, Serialize, ToSchema, serde::Deserialize)]
pub struct TaskCommentResponse {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub task_id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub user_id: Uuid,
    pub body: String,
    #[schema(value_type = Option<String>, format = "uuid", nullable)]
    pub parent_comment_id: Option<Uuid>,
    #[schema(value_type = String, format = "date-time")]
    pub created_at: DateTime<Utc>,
    #[schema(value_type = String, format = "date-time")]
    pub updated_at: DateTime<Utc>,
    #[schema(value_type = Option<String>, format = "date-time", nullable)]
    pub deleted_at: Option<DateTime<Utc>>,
}

impl From<task_comments::Model> for TaskCommentResponse {
    fn from(model: task_comments::Model) -> Self {
        Self {
            id: model.id,
            task_id: model.task_id,
            user_id: model.user_id,
            body: model.body,
            parent_comment_id: model.parent_comment_id,
            created_at: model.created_at.with_timezone(&Utc),
            updated_at: model.updated_at.with_timezone(&Utc),
            deleted_at: model.deleted_at.map(|dt| dt.with_timezone(&Utc)),
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct CommentUser {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub name: String,
    /// 画面でアイコンを出すために返す。未設定なら頭文字で描く
    #[schema(nullable)]
    pub avatar_url: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct CommentReply {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub user: CommentUser,
    #[schema(nullable)]
    pub body: Option<String>,
    #[schema(value_type = String, format = "date-time")]
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[schema(value_type = String, format = "date-time")]
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub is_deleted: bool,
}

#[derive(Serialize, ToSchema)]
pub struct CommentThread {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub user: CommentUser,
    #[schema(nullable)]
    pub body: Option<String>,
    pub replies: Vec<CommentReply>,
    #[schema(value_type = String, format = "date-time")]
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[schema(value_type = String, format = "date-time")]
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub is_deleted: bool,
}

#[derive(Serialize, ToSchema)]
pub struct CommentListResponse {
    pub comments: Vec<CommentThread>,
}

#[derive(Serialize, ToSchema)]
pub struct ActivityUser {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub name: String,
}

#[derive(Serialize, ToSchema)]
pub struct ActivityItem {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub event_type: String,
    #[schema(nullable)]
    pub user: Option<ActivityUser>,
    #[schema(value_type = serde_json::Value)]
    pub payload: serde_json::Value,
    #[schema(value_type = String, format = "date-time")]
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// 履歴の取得範囲。
///
/// 履歴は操作のたびに増えるので、既定で先頭だけ返す。全件返すと
/// 長く使われたタスクほど DB・レスポンス・描画のコストが上限なく伸びる。
#[derive(Deserialize, ToSchema, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListActivitiesQuery {
    #[serde(default = "default_activities_limit")]
    pub limit: u64,
    /// 前のページの `next_cursor`。先頭ページでは付けない。
    ///
    /// `offset` を使わないのは、履歴が積まれている最中にページを継ぐと境界がずれ、
    /// 同じ行が 2 度出たり抜けたりするため（`common::cursor`）。
    pub cursor: Option<String>,
}

fn default_activities_limit() -> u64 {
    20
}

/// 上限。これより大きい `limit` は切り詰める。
pub const MAX_ACTIVITIES_LIMIT: u64 = 100;

#[derive(Serialize, ToSchema)]
pub struct ActivityListResponse {
    pub activities: Vec<ActivityItem>,
    /// 総数。件数の表示に使う。**「まだ残っているか」の判断には使わない**
    /// （取得中に増えるので、総数と取得済み件数の比較では終わらなくなる）
    pub total: u64,
    /// 次のページを引く鍵。`null` なら取り切っている
    #[schema(required, nullable)]
    pub next_cursor: Option<String>,
}

#[derive(Validate, Deserialize, ToSchema, serde::Serialize)]
pub struct CreateCommentRequest {
    #[validate(length(min = 1))]
    pub body: String,
    #[schema(value_type = Option<String>, format = "uuid")]
    pub parent_comment_id: Option<Uuid>,
}

#[derive(Validate, Deserialize, ToSchema)]
pub struct UpdateCommentRequest {
    #[validate(length(min = 1))]
    pub body: String,
}
