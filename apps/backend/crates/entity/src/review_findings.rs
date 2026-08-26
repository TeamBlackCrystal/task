//! Review findings entity — schema-first with hand-written DeriveActiveEnum.
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// 指摘の重大度。`High` / `Medium` はマージ前必須、`Low` / `Nit` は繰り延べ可。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize, Deserialize, ToSchema,
)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(255))")]
#[serde(rename_all = "lowercase")]
pub enum FindingSeverity {
    #[sea_orm(string_value = "high")]
    High,
    #[sea_orm(string_value = "medium")]
    Medium,
    #[sea_orm(string_value = "low")]
    Low,
    #[sea_orm(string_value = "nit")]
    Nit,
}

impl FindingSeverity {
    /// マージ前に解消が必要な重大度か。
    pub fn blocks_merge(self) -> bool {
        matches!(self, Self::High | Self::Medium)
    }

    /// API・メッセージで使う表記（`FromStr` の裏返し）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
            Self::Nit => "nit",
        }
    }

    /// 繰り延べ（`deferred`）を許す重大度か。
    ///
    /// `deferred` はマージ可否の集計から外れる状態なので、マージ前必須の重大度に
    /// 許すとマージ基準そのものを迂回できてしまう（仕様 §3）。
    pub fn can_defer(self) -> bool {
        !self.blocks_merge()
    }
}

impl std::str::FromStr for FindingSeverity {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "high" => Ok(Self::High),
            "medium" => Ok(Self::Medium),
            "low" => Ok(Self::Low),
            "nit" => Ok(Self::Nit),
            _ => Err(()),
        }
    }
}

/// 指摘の状態。
///
/// 遷移規則は `service::reviews::can_transition` を正とする。`Verified` は終端で、
/// 誤りだったと分かった場合は新しいラウンドで指摘を出し直す。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize, Deserialize, ToSchema,
)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(255))")]
#[serde(rename_all = "lowercase")]
pub enum FindingState {
    #[sea_orm(string_value = "open")]
    Open,
    #[sea_orm(string_value = "fixed")]
    Fixed,
    #[sea_orm(string_value = "verified")]
    Verified,
    #[sea_orm(string_value = "deferred")]
    Deferred,
    #[sea_orm(string_value = "rejected")]
    Rejected,
}

impl FindingState {
    /// API・メッセージで使う表記（`FromStr` の裏返し）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Fixed => "fixed",
            Self::Verified => "verified",
            Self::Deferred => "deferred",
            Self::Rejected => "rejected",
        }
    }

    /// マージ判定で「未解決」と数える状態か。
    ///
    /// `Fixed` を未解決に数えるのは、修正の宣言だけでは確認が済んでいないため
    /// （仕様 §5 の集計と同じ規則）。
    pub fn counts_as_unresolved(self) -> bool {
        matches!(self, Self::Open | Self::Fixed)
    }
}

impl std::str::FromStr for FindingState {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "open" => Ok(Self::Open),
            "fixed" => Ok(Self::Fixed),
            "verified" => Ok(Self::Verified),
            "deferred" => Ok(Self::Deferred),
            "rejected" => Ok(Self::Rejected),
            _ => Err(()),
        }
    }
}

pub use super::_generated::review_findings::*;
