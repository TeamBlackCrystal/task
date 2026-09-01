use sea_orm::prelude::Uuid;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

use entity::labels;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct LabelResponse {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub color: String,
    #[schema(nullable)]
    pub icon_url: Option<String>,
    #[schema(value_type = Option<String>, format = "uuid", nullable)]
    pub project_id: Option<Uuid>,
}

impl From<labels::Model> for LabelResponse {
    fn from(model: labels::Model) -> Self {
        Self {
            id: model.id,
            name: model.name,
            description: model.description,
            color: model.color,
            icon_url: model.icon_url,
            project_id: model.project_id,
        }
    }
}

#[derive(Validate, Deserialize, ToSchema)]
pub struct CreateLabelRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[validate(regex(path = "common::validation::COLOR_REGEX"))]
    pub color: String,
    pub icon_url: Option<String>,
}

#[derive(Validate, Deserialize, ToSchema)]
pub struct UpdateLabelRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: Option<String>,
    pub description: Option<String>,
    #[validate(regex(path = "common::validation::COLOR_REGEX"))]
    pub color: Option<String>,
    pub icon_url: Option<String>,
    #[serde(default)]
    pub clear_icon_url: bool,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct LabelExport {
    pub version: u32,
    pub labels: Vec<LabelExportItem>,
}

#[derive(Validate, Serialize, Deserialize, ToSchema)]
pub struct LabelExportItem {
    #[validate(length(min = 1, max = 100))]
    pub name: String,
    #[validate(regex(path = "common::validation::COLOR_REGEX"))]
    pub color: String,
    pub description: String,
}

#[derive(Validate, Deserialize, ToSchema)]
pub struct ImportLabelRequest {
    pub version: u32,
    #[validate(length(max = 500))]
    pub labels: Vec<LabelExportItem>,
    #[serde(default)]
    pub on_conflict: ImportConflict,
}

#[derive(Deserialize, ToSchema, Default)]
#[serde(rename_all = "lowercase")]
pub enum ImportConflict {
    #[default]
    Skip,
    Overwrite,
}
