use sea_orm::FromJsonQueryResult;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum Scope {
    #[serde(rename = "read:project")]
    ReadProject,
    #[serde(rename = "write:project")]
    WriteProject,
    #[serde(rename = "read:drive")]
    ReadDrive,
    #[serde(rename = "write:drive")]
    WriteDrive,
    #[serde(rename = "admin:tenant")]
    AdminTenant,
    #[serde(rename = "read:task")]
    ReadTask,
    #[serde(rename = "write:task")]
    WriteTask,
    #[serde(rename = "read:milestone")]
    ReadMilestone,
    #[serde(rename = "write:milestone")]
    WriteMilestone,
    #[serde(rename = "read:sprint")]
    ReadSprint,
    #[serde(rename = "write:sprint")]
    WriteSprint,
    #[serde(rename = "read:review")]
    ReadReview,
    #[serde(rename = "write:review")]
    WriteReview,
}

impl Scope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Scope::ReadProject => "read:project",
            Scope::WriteProject => "write:project",
            Scope::ReadDrive => "read:drive",
            Scope::WriteDrive => "write:drive",
            Scope::AdminTenant => "admin:tenant",
            Scope::ReadTask => "read:task",
            Scope::WriteTask => "write:task",
            Scope::ReadMilestone => "read:milestone",
            Scope::WriteMilestone => "write:milestone",
            Scope::ReadSprint => "read:sprint",
            Scope::WriteSprint => "write:sprint",
            Scope::ReadReview => "read:review",
            Scope::WriteReview => "write:review",
        }
    }
}

impl std::str::FromStr for Scope {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "read:project" => Ok(Scope::ReadProject),
            "write:project" => Ok(Scope::WriteProject),
            "read:drive" => Ok(Scope::ReadDrive),
            "write:drive" => Ok(Scope::WriteDrive),
            "admin:tenant" => Ok(Scope::AdminTenant),
            "read:task" => Ok(Scope::ReadTask),
            "write:task" => Ok(Scope::WriteTask),
            "read:milestone" => Ok(Scope::ReadMilestone),
            "write:milestone" => Ok(Scope::WriteMilestone),
            "read:sprint" => Ok(Scope::ReadSprint),
            "write:sprint" => Ok(Scope::WriteSprint),
            "read:review" => Ok(Scope::ReadReview),
            "write:review" => Ok(Scope::WriteReview),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// アクセストークン等に付与する権限スコープのリスト。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, FromJsonQueryResult, ToSchema)]
#[serde(transparent)]
pub struct ScopeList(pub Vec<Scope>);

impl ScopeList {
    pub fn has_scope(&self, scope: Scope) -> bool {
        self.0.contains(&scope)
            || self.0.contains(&Scope::AdminTenant)
            || (scope == Scope::ReadDrive && self.0.contains(&Scope::WriteDrive))
            || (scope == Scope::ReadTask && self.0.contains(&Scope::WriteTask))
            || (scope == Scope::ReadMilestone && self.0.contains(&Scope::WriteMilestone))
            || (scope == Scope::ReadSprint && self.0.contains(&Scope::WriteSprint))
            || (scope == Scope::ReadReview && self.0.contains(&Scope::WriteReview))
    }
}

impl From<Scope> for sea_orm::Value {
    fn from(source: Scope) -> Self {
        sea_orm::Value::String(Some(source.as_str().to_string()))
    }
}
