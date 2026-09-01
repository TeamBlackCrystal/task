use sea_orm::prelude::Uuid;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

use common::notifications::KNOWN_EVENT_TYPES;

fn validate_known_event_types(events: &Vec<String>) -> Result<(), validator::ValidationError> {
    for e in events {
        if !KNOWN_EVENT_TYPES.contains(&e.as_str()) {
            return Err(validator::ValidationError::new("unknown_event_type"));
        }
    }
    Ok(())
}

#[derive(Serialize, ToSchema)]
pub struct WatcherUser {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub name: String,
    #[schema(value_type = String, format = "date-time")]
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize, ToSchema)]
pub struct WatcherListResponse {
    pub watchers: Vec<WatcherUser>,
}

#[derive(Serialize, ToSchema)]
pub struct NotificationTaskSummary {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub seq_id: i32,
    pub title: String,
}

#[derive(Serialize, ToSchema)]
pub struct NotificationItem {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub notification_type: String,
    #[schema(nullable)]
    pub task: Option<NotificationTaskSummary>,
    #[schema(value_type = serde_json::Value)]
    pub payload: serde_json::Value,
    #[schema(nullable, value_type = Option<String>, format = "date-time")]
    pub read_at: Option<chrono::DateTime<chrono::Utc>>,
    #[schema(value_type = String, format = "date-time")]
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize, ToSchema)]
pub struct NotificationListResponse {
    pub unread_count: u64,
    pub notifications: Vec<NotificationItem>,
}

#[derive(Deserialize, ToSchema)]
pub struct ListNotificationsQuery {
    pub unread: Option<bool>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

#[derive(Serialize, ToSchema)]
pub struct NotificationSettingsResponse {
    pub email_events: Vec<String>,
    pub in_app_events: Vec<String>,
}

#[derive(Validate, Deserialize, ToSchema)]
pub struct UpdateNotificationSettingsRequest {
    #[validate(custom(function = "validate_known_event_types"))]
    pub email_events: Vec<String>,
    #[validate(custom(function = "validate_known_event_types"))]
    pub in_app_events: Vec<String>,
}
