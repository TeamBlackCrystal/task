use chrono::{DateTime, Utc};
use sea_orm::prelude::Uuid;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

use crate::users::UserSummary;
use entity::review_findings::{FindingSeverity, FindingState};
use entity::{review_finding_transitions, review_findings, reviews};

/// 1 ラウンドで作れる指摘の上限。1 リクエストの一括作成で無制限に積ませない。
pub const MAX_FINDINGS_PER_ROUND: u64 = 200;

// ── リクエスト ──────────────────────────────────────────────────────────

/// ラウンド 1 回ぶんの一括起票。ラウンドと指摘は同じトランザクションで作る。
#[derive(Validate, Debug, Deserialize, ToSchema)]
pub struct CreateReviewRequest {
    /// 対象 PR 番号
    #[validate(range(min = 1))]
    pub pr_number: i32,
    /// レビューした commit。裏取りした head の記録として必須
    #[validate(length(min = 1, max = 100))]
    pub head_sha: String,
    /// 総評（markdown）
    #[serde(default)]
    #[validate(length(max = 20000))]
    pub summary: String,
    /// 指摘。空配列も正当（「指摘なし」の記録）
    #[serde(default)]
    #[validate(length(max = "MAX_FINDINGS_PER_ROUND"), nested)]
    pub findings: Vec<CreateFindingInput>,
}

#[derive(Validate, Debug, Serialize, Deserialize, ToSchema)]
pub struct CreateFindingInput {
    pub severity: FindingSeverity,
    #[validate(length(min = 1, max = 200))]
    pub title: String,
    #[validate(length(min = 1, max = 20000))]
    pub body: String,
    #[schema(nullable)]
    #[validate(length(max = 1000))]
    pub file: Option<String>,
    #[schema(nullable)]
    #[validate(range(min = 1))]
    pub line: Option<i32>,
}

/// 指摘の状態遷移。
#[derive(Validate, Debug, Deserialize, ToSchema)]
pub struct UpdateFindingStateRequest {
    pub state: FindingState,
    /// 遷移の理由（履歴に残す）
    #[schema(nullable)]
    #[validate(length(max = 2000))]
    pub note: Option<String>,
}

// ── レスポンス ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ReviewResponse {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub project_id: Uuid,
    pub pr_number: i32,
    /// PR 内の連番（1 始まり）。表示は R1, R2, …
    pub round: i32,
    pub head_sha: String,
    pub reviewer: UserSummary,
    pub summary: String,
    /// 要約コメント投稿時に GitHub から取得してキャッシュした PR タイトル
    #[schema(nullable)]
    pub pr_title: Option<String>,
    #[schema(nullable)]
    pub pr_author: Option<String>,
    #[schema(value_type = String, format = "date-time")]
    pub created_at: DateTime<Utc>,
    /// このラウンドで出した指摘の件数
    pub finding_count: u64,
}

impl ReviewResponse {
    pub fn from_parts(model: reviews::Model, reviewer: entity::users::Model, count: u64) -> Self {
        Self {
            id: model.id,
            project_id: model.project_id,
            pr_number: model.pr_number,
            round: model.round,
            head_sha: model.head_sha,
            reviewer: reviewer.into(),
            summary: model.summary,
            pr_title: model.pr_title,
            pr_author: model.pr_author,
            created_at: model.created_at.with_timezone(&Utc),
            finding_count: count,
        }
    }
}

/// ラウンドと、そのラウンドで出した指摘。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ReviewDetailResponse {
    #[serde(flatten)]
    pub review: ReviewResponse,
    pub findings: Vec<FindingResponse>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct FindingResponse {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub review_id: Uuid,
    pub pr_number: i32,
    pub round: i32,
    pub severity: FindingSeverity,
    pub title: String,
    pub body: String,
    #[schema(nullable)]
    pub file: Option<String>,
    #[schema(nullable)]
    pub line: Option<i32>,
    pub state: FindingState,
    /// 繰り延べ時に自動起票した通常タスク
    #[schema(value_type = Option<String>, format = "uuid", nullable)]
    pub deferred_task_id: Option<Uuid>,
    /// `fixed` を宣言した利用者。`verified` に進めてよいかの判定に使う
    #[schema(value_type = Option<String>, format = "uuid", nullable)]
    pub fixed_by: Option<Uuid>,
    #[schema(value_type = String, format = "date-time")]
    pub created_at: DateTime<Utc>,
    #[schema(value_type = String, format = "date-time")]
    pub updated_at: DateTime<Utc>,
    pub transitions: Vec<FindingTransitionResponse>,
}

impl FindingResponse {
    pub fn from_parts(
        model: review_findings::Model,
        pr_number: i32,
        round: i32,
        transitions: Vec<FindingTransitionResponse>,
    ) -> Self {
        Self {
            id: model.id,
            review_id: model.review_id,
            pr_number,
            round,
            severity: model.severity,
            title: model.title,
            body: model.body,
            file: model.file,
            line: model.line,
            state: model.state,
            deferred_task_id: model.deferred_task_id,
            fixed_by: model.fixed_by,
            created_at: model.created_at.with_timezone(&Utc),
            updated_at: model.updated_at.with_timezone(&Utc),
            transitions,
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct FindingTransitionResponse {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub actor: UserSummary,
    /// 起票（登録）は `null`
    #[schema(nullable)]
    pub from_state: Option<FindingState>,
    pub to_state: FindingState,
    #[schema(nullable)]
    pub note: Option<String>,
    #[schema(value_type = String, format = "date-time")]
    pub created_at: DateTime<Utc>,
}

impl FindingTransitionResponse {
    pub fn from_parts(
        model: review_finding_transitions::Model,
        actor: entity::users::Model,
    ) -> Self {
        Self {
            id: model.id,
            actor: actor.into(),
            from_state: model.from_state,
            to_state: model.to_state,
            note: model.note,
            created_at: model.created_at.with_timezone(&Utc),
        }
    }
}

/// PR 単位の集計。マージ可否をこの 1 レスポンスで判断できる。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ReviewSummaryResponse {
    pub pr_number: i32,
    /// これまでに走ったラウンド数（R1, R2, … の最大値）
    pub rounds: i32,
    /// 重大度 × 状態の件数
    pub counts: Vec<SeverityStateCount>,
    /// マージ判定を塞いでいる指摘の件数（High / Medium かつ open / fixed）
    pub blocking: u64,
    /// 最新ラウンドがレビューした commit。呼び出し側が現在の HEAD と突き合わせる
    #[schema(nullable)]
    pub latest_head_sha: Option<String>,
    /// 集計対象のリポジトリ（`owner/name`）。GitHub 連携が無ければ `null`
    ///
    /// 連携を外すと集計の視界が空になるので、ゲートとして使う側は
    /// これが `null` の集計を通してはいけない（仕様 §5 / §6）。
    #[schema(nullable)]
    pub repository: Option<String>,
    /// ラウンドが 1 件以上あり、かつ `blocking == 0` か
    ///
    /// レビューが 1 件も無い PR を「可」にしない（未レビューと「指摘なし」は違う）。
    pub mergeable: bool,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SeverityStateCount {
    pub severity: FindingSeverity,
    pub state: FindingState,
    pub count: u64,
}
