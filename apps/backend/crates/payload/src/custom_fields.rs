use chrono::{DateTime, Utc};
use sea_orm::prelude::Uuid;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use validator::Validate;

use entity::project_custom_fields;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProjectCustomFieldResponse {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub project_id: Uuid,
    pub name: String,
    pub field_type: project_custom_fields::CustomFieldType,
    #[schema(nullable)]
    pub options: Option<Value>,
    pub is_required: bool,
    pub position: i16,
    #[schema(value_type = String, format = "date-time")]
    pub created_at: DateTime<Utc>,
}

impl From<project_custom_fields::Model> for ProjectCustomFieldResponse {
    fn from(model: project_custom_fields::Model) -> Self {
        Self {
            id: model.id,
            project_id: model.project_id,
            name: model.name,
            field_type: model.field_type,
            options: model.options,
            is_required: model.is_required,
            position: model.position,
            created_at: model.created_at.with_timezone(&Utc),
        }
    }
}

#[derive(Validate, Deserialize, ToSchema)]
pub struct CreateCustomFieldRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: String,
    pub field_type: project_custom_fields::CustomFieldType,
    pub options: Option<Value>,
    #[serde(default)]
    pub is_required: bool,
    pub position: Option<i16>,
}

#[derive(Validate, Deserialize, ToSchema)]
pub struct UpdateCustomFieldRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: Option<String>,
    pub options: Option<Value>,
    pub is_required: Option<bool>,
    pub position: Option<i16>,
}

#[derive(Serialize, ToSchema)]
pub struct CustomFieldListResponse {
    pub fields: Vec<ProjectCustomFieldResponse>,
}

#[derive(Serialize, ToSchema)]
pub struct CustomFieldDefinitionSummary {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub name: String,
    pub field_type: project_custom_fields::CustomFieldType,
    pub is_required: bool,
    pub position: i16,
}

impl From<&project_custom_fields::Model> for CustomFieldDefinitionSummary {
    fn from(field: &project_custom_fields::Model) -> Self {
        Self {
            id: field.id,
            name: field.name.clone(),
            field_type: field.field_type,
            is_required: field.is_required,
            position: field.position,
        }
    }
}

#[derive(Deserialize, ToSchema)]
pub struct CustomFieldValueInput {
    #[schema(value_type = String, format = "uuid")]
    pub field_id: Uuid,
    #[schema(nullable)]
    pub value: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct TaskCustomFieldValueResponse {
    pub field: CustomFieldDefinitionSummary,
    #[schema(nullable)]
    pub value: Option<String>,
    #[schema(nullable)]
    pub display_value: Option<String>,
}
