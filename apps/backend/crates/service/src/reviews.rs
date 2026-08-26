//! レビューラウンドと指摘の業務ロジック。
//!
//! 仕様は `docs/features/review-findings.md`。ここに置くのは
//! 「ラウンドの採番」「状態遷移の規則」「繰り延べ先タスクの作成・クローズ」。

use sea_orm::sea_query::LockType;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect, prelude::Uuid,
};

use entity::review_findings::{FindingSeverity, FindingState};
use entity::{project_statuses, projects, review_findings, reviews, tasks};

/// 繰り延べで自動起票するタスクのタイトル接頭辞。
const DEFERRED_TASK_PREFIX: &str = "[レビュー指摘]";

#[derive(Debug, thiserror::Error)]
pub enum ReviewError {
    #[error("invalid transition: {from:?} -> {to:?}")]
    InvalidTransition {
        from: FindingState,
        to: FindingState,
    },
    /// レビュー側だけが行える遷移を、修正側が行おうとした。
    #[error("transition requires the reviewer side")]
    ReviewerOnly,
    /// `fixed` を宣言した本人が `verified` に進めようとした。
    #[error("the fixer cannot verify their own fix")]
    SelfVerification,
    /// マージ前必須の重大度を繰り延べようとした。
    #[error("severity {0:?} cannot be deferred")]
    NotDeferrable(FindingSeverity),
    #[error("project {0} has no default status; cannot create the deferred task")]
    NoDefaultStatus(Uuid),
    #[error(transparent)]
    Db(#[from] sea_orm::DbErr),
}

impl From<ReviewError> for common::error::AppError {
    fn from(err: ReviewError) -> Self {
        // 409 は理由を本文に入れる。共通の `conflict` だけでは、CLI から使う
        // レビュワー（AI を含む）が「なぜ通らないのか」を判断できない
        match err {
            // 現在の状態からは行えない遷移。入力の形式は正しいので 409
            ReviewError::InvalidTransition { from, to } => Self::ConflictDetail(format!(
                "{} の指摘を {} にはできません",
                from.as_str(),
                to.as_str()
            )),
            // 遷移そのものは規則にあるが、この指摘の重大度では行えない。
            // 入力の形式は正しいので InvalidTransition と同じ 409
            ReviewError::NotDeferrable(severity) => Self::ConflictDetail(format!(
                "{} の指摘は繰り延べられません（繰り延べは low / nit のみ。マージ前に解消するか、指摘自体を取り下げてください）",
                severity.as_str()
            )),
            ReviewError::ReviewerOnly | ReviewError::SelfVerification => Self::Forbidden,
            // 既定ステータスが無いプロジェクトでは繰り延べ先タスクを作れない。
            // 利用者が直せる状態の問題なので 409（指摘の状態は変えない）
            ReviewError::NoDefaultStatus(_) => Self::ConflictDetail(
                "プロジェクトに既定ステータスが無いため、繰り延べ先のタスクを作れません".into(),
            ),
            ReviewError::Db(err) => err.into(),
        }
    }
}

/// PR 内の次のラウンド番号を返す。
///
/// 同じ PR に同時にラウンドを作られると `UNIQUE (project_id, pr_number, round)` に
/// ぶつかるため、プロジェクト行を掴んで採番から挿入までを直列化する。
/// レビューの起票は頻度が低く、この粒度で待たせても実害がない。
pub async fn next_round<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    pr_number: i32,
) -> Result<i32, sea_orm::DbErr> {
    projects::Entity::find_by_id(project_id)
        .lock(LockType::Update)
        .one(db)
        .await?;

    let last: Option<i32> = reviews::Entity::find()
        .filter(reviews::Column::ProjectId.eq(project_id))
        .filter(reviews::Column::PrNumber.eq(pr_number))
        .select_only()
        .column(reviews::Column::Round)
        .order_by_desc(reviews::Column::Round)
        .into_tuple()
        .one(db)
        .await?;

    Ok(last.unwrap_or(0) + 1)
}

/// 状態遷移そのものが許されるか（誰が行うかは [`requires_reviewer_side`] で見る）。
///
/// `verified` は終端。誤りだったと分かった場合は新しいラウンドで指摘を出し直す。
pub fn can_transition(from: FindingState, to: FindingState) -> bool {
    use FindingState::*;
    match (from, to) {
        // 修正の宣言と、その確認
        (Open, Fixed) | (Fixed, Verified) => true,
        // 繰り延べと取り消し（繰り延べられる重大度は [`FindingSeverity::can_defer`] で見る）
        (Open, Deferred) | (Deferred, Open) => true,
        // 指摘自体の棄却と再オープン
        (Open, Rejected) | (Rejected, Open) => true,
        // 差し戻し（再確認で未修正と判断）
        (Fixed, Open) => true,
        _ => false,
    }
}

/// その遷移がレビュー側（ラウンドの作成者、または同じ PR のより新しい
/// ラウンドの作成者）に限られるか。
///
/// `fixed`（修正の宣言）と `deferred` からの復帰は修正側も行える。
pub fn requires_reviewer_side(from: FindingState, to: FindingState) -> bool {
    use FindingState::*;
    matches!(
        (from, to),
        (Fixed, Verified) | (Fixed, Open) | (Open, Rejected) | (Rejected, Open)
    )
}

/// `actor` が対象 PR のレビュー側か。
///
/// 「その指摘を含むラウンドの作成者」か「同じ PR のより新しいラウンドの作成者」。
/// 修正だけを行う利用者を締め出すのが目的で、レビューを一度でも出した人は
/// 以後の確認も行える。
pub async fn is_reviewer_side<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    pr_number: i32,
    round: i32,
    actor_id: Uuid,
) -> Result<bool, sea_orm::DbErr> {
    let found = reviews::Entity::find()
        .filter(reviews::Column::ProjectId.eq(project_id))
        .filter(reviews::Column::PrNumber.eq(pr_number))
        .filter(reviews::Column::Round.gte(round))
        .filter(reviews::Column::ReviewerId.eq(actor_id))
        .one(db)
        .await?;
    Ok(found.is_some())
}

/// 遷移の可否を判定する。DB は読むが書かない。
///
/// - 遷移そのものが規則にない → [`ReviewError::InvalidTransition`]
/// - マージ前必須の重大度を繰り延べようとした → [`ReviewError::NotDeferrable`]
/// - レビュー側限定の遷移を修正側が行った → [`ReviewError::ReviewerOnly`]
/// - `fixed` を宣言した本人が `verified` に進めた → [`ReviewError::SelfVerification`]
pub async fn ensure_transition_allowed<C: ConnectionTrait>(
    db: &C,
    finding: &review_findings::Model,
    review: &reviews::Model,
    to: FindingState,
    actor_id: Uuid,
) -> Result<(), ReviewError> {
    let from = finding.state;
    if !can_transition(from, to) {
        return Err(ReviewError::InvalidTransition { from, to });
    }

    // 繰り延べはマージ可否の集計から外れるので、High / Medium には許さない
    // （許すと「High を deferred にしてマージ可」という迂回路ができる）
    if to == FindingState::Deferred && !finding.severity.can_defer() {
        return Err(ReviewError::NotDeferrable(finding.severity));
    }

    if to == FindingState::Verified && finding.fixed_by == Some(actor_id) {
        return Err(ReviewError::SelfVerification);
    }

    if requires_reviewer_side(from, to)
        && !is_reviewer_side(
            db,
            review.project_id,
            review.pr_number,
            review.round,
            actor_id,
        )
        .await?
    {
        return Err(ReviewError::ReviewerOnly);
    }

    Ok(())
}

/// 繰り延べ先の通常タスクを起票する。
///
/// 指摘の内容をタスク本文に写し、優先度は Low 固定。ステータスはプロジェクトの
/// 既定ステータス（無ければエラー）。
pub async fn create_deferred_task<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    finding: &review_findings::Model,
    review: &reviews::Model,
    actor_id: Uuid,
) -> Result<tasks::Model, ReviewError> {
    let status = project_statuses::Entity::find()
        .filter(project_statuses::Column::ProjectId.eq(project_id))
        .filter(project_statuses::Column::IsDefault.eq(true))
        .order_by_asc(project_statuses::Column::Position)
        .one(db)
        .await?
        .ok_or(ReviewError::NoDefaultStatus(project_id))?;

    let seq_id = crate::tasks::next_seq_id(db, project_id).await?;
    let now = chrono::Utc::now();

    let location = match (&finding.file, finding.line) {
        (Some(file), Some(line)) => format!("\n\n対象: `{file}:{line}`"),
        (Some(file), None) => format!("\n\n対象: `{file}`"),
        _ => String::new(),
    };
    let description = format!(
        "PR #{} R{} のレビュー指摘（{:?}）を繰り延べたタスク。\n\n{}{}",
        review.pr_number, review.round, finding.severity, finding.body, location
    );

    let task = tasks::ActiveModel {
        id: Set(Uuid::new_v4()),
        project_id: Set(project_id),
        seq_id: Set(seq_id),
        title: Set(format!("{DEFERRED_TASK_PREFIX} {}", finding.title)),
        description: Set(Some(description)),
        status_id: Set(status.id),
        priority: Set(tasks::TaskPriority::Low),
        progress_pct: Set(0),
        parent_task_id: Set(None),
        milestone_id: Set(None),
        sprint_id: Set(None),
        soft_deadline: Set(None),
        hard_deadline: Set(None),
        estimated_minutes: Set(None),
        is_archived: Set(false),
        created_by: Set(actor_id),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
        completed_at: Set(None),
        deleted_at: Set(None),
    }
    .insert(db)
    .await?;

    Ok(task)
}

/// 繰り延べを取り消すとき、自動起票したタスクを完了させる。
///
/// 二重管理を作らないための後始末。既に消えている・見つからない場合は何もしない
/// （指摘側のリンクは呼び出し側が NULL に戻す）。
pub async fn close_deferred_task<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    task_id: Uuid,
) -> Result<(), ReviewError> {
    let Some(task) = tasks::Entity::find_by_id(task_id)
        .filter(tasks::Column::ProjectId.eq(project_id))
        .filter(tasks::Column::DeletedAt.is_null())
        .one(db)
        .await?
    else {
        return Ok(());
    };

    let done = project_statuses::Entity::find()
        .filter(project_statuses::Column::ProjectId.eq(project_id))
        .filter(project_statuses::Column::IsDoneState.eq(true))
        .order_by_asc(project_statuses::Column::Position)
        .one(db)
        .await?;

    let now = chrono::Utc::now();
    let mut active: tasks::ActiveModel = task.into();
    // 完了ステータスがあれば完了に、無ければソフト削除で残さない
    match done {
        Some(status) => {
            active.status_id = Set(status.id);
            active.completed_at = Set(Some(now.into()));
        }
        None => {
            active.deleted_at = Set(Some(now.into()));
        }
    }
    active.updated_at = Set(now.into());
    active.update(db).await?;
    Ok(())
}

/// 指摘 1 件を新しい状態へ進める（履歴の記録込み）。
///
/// 呼び出し側はトランザクションを渡すこと。繰り延べ先タスクの作成・クローズが
/// 失敗したら指摘の状態も変えない（仕様 §10 の「不整合を作らない」）。
pub async fn apply_transition<C: ConnectionTrait>(
    db: &C,
    finding: review_findings::Model,
    review: &reviews::Model,
    to: FindingState,
    actor_id: Uuid,
    note: Option<String>,
) -> Result<review_findings::Model, ReviewError> {
    ensure_transition_allowed(db, &finding, review, to, actor_id).await?;

    let from = finding.state;
    let now = chrono::Utc::now();

    // 繰り延べの出入りで、リンク先タスクを作る／畳む
    let mut deferred_task_id = finding.deferred_task_id;
    if to == FindingState::Deferred {
        let task = create_deferred_task(db, review.project_id, &finding, review, actor_id).await?;
        deferred_task_id = Some(task.id);
    } else if from == FindingState::Deferred {
        if let Some(task_id) = finding.deferred_task_id {
            close_deferred_task(db, review.project_id, task_id).await?;
        }
        deferred_task_id = None;
    }

    let finding_id = finding.id;
    let mut active: review_findings::ActiveModel = finding.into();
    active.state = Set(to);
    active.deferred_task_id = Set(deferred_task_id);
    // 誰が直したかは verified の判定に使うので、fixed を出るときに消さない
    // （差し戻し後に同じ人が verified へ進めるのを防ぐ）
    if to == FindingState::Fixed {
        active.fixed_by = Set(Some(actor_id));
    }
    active.updated_at = Set(now.into());
    let updated = active.update(db).await?;

    record_transition(db, finding_id, actor_id, Some(from), to, note).await?;

    Ok(updated)
}

/// 遷移履歴を 1 行残す。`from` が `None` の行は起票（登録）を表す。
pub async fn record_transition<C: ConnectionTrait>(
    db: &C,
    finding_id: Uuid,
    actor_id: Uuid,
    from: Option<FindingState>,
    to: FindingState,
    note: Option<String>,
) -> Result<(), sea_orm::DbErr> {
    entity::review_finding_transitions::ActiveModel {
        id: Set(Uuid::new_v4()),
        finding_id: Set(finding_id),
        actor_id: Set(actor_id),
        from_state: Set(from),
        to_state: Set(to),
        note: Set(note),
        created_at: Set(chrono::Utc::now().into()),
    }
    .insert(db)
    .await?;
    Ok(())
}

/// PR 単位の集計（重大度 × 状態の件数）。
pub async fn severity_state_counts<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    pr_number: i32,
) -> Result<Vec<(FindingSeverity, FindingState, u64)>, sea_orm::DbErr> {
    let rows: Vec<(FindingSeverity, FindingState, i64)> = review_findings::Entity::find()
        .inner_join(reviews::Entity)
        .filter(reviews::Column::ProjectId.eq(project_id))
        .filter(reviews::Column::PrNumber.eq(pr_number))
        .select_only()
        .column(review_findings::Column::Severity)
        .column(review_findings::Column::State)
        .column_as(review_findings::Column::Id.count(), "count")
        .group_by(review_findings::Column::Severity)
        .group_by(review_findings::Column::State)
        .into_tuple()
        .all(db)
        .await?;

    Ok(rows
        .into_iter()
        .map(|(severity, state, count)| (severity, state, count.max(0) as u64))
        .collect())
}

/// マージを塞いでいる指摘の件数（High / Medium かつ open / fixed）。
pub fn blocking_count(counts: &[(FindingSeverity, FindingState, u64)]) -> u64 {
    counts
        .iter()
        .filter(|(severity, state, _)| severity.blocks_merge() && state.counts_as_unresolved())
        .map(|(_, _, count)| *count)
        .sum()
}

/// PR 内のラウンド数（= 最大の round）。
pub async fn round_count<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    pr_number: i32,
) -> Result<i32, sea_orm::DbErr> {
    let last: Option<i32> = reviews::Entity::find()
        .filter(reviews::Column::ProjectId.eq(project_id))
        .filter(reviews::Column::PrNumber.eq(pr_number))
        .select_only()
        .column(reviews::Column::Round)
        .order_by_desc(reviews::Column::Round)
        .into_tuple()
        .one(db)
        .await?;
    Ok(last.unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use FindingState::*;

    /// 遷移表そのものの固定。仕様 §3 の図と 1:1 で対応させる。
    #[test]
    fn transition_table_matches_the_spec() {
        let allowed = [
            (Open, Fixed),
            (Fixed, Verified),
            (Fixed, Open),
            (Open, Deferred),
            (Deferred, Open),
            (Open, Rejected),
            (Rejected, Open),
        ];
        for (from, to) in allowed {
            assert!(can_transition(from, to), "{from:?} -> {to:?} は許される");
        }

        // verified は終端。誤りは新しいラウンドで出し直す
        for to in [Open, Fixed, Deferred, Rejected] {
            assert!(
                !can_transition(Verified, to),
                "verified -> {to:?} は許されない"
            );
        }
        // 確認を飛ばして verified にはできない
        assert!(!can_transition(Open, Verified));
        // 繰り延べたものを直接 fixed にはできない（一度 open へ戻す）
        assert!(!can_transition(Deferred, Fixed));
        // 同じ状態への遷移は不可（履歴だけが増えるのを防ぐ）
        for state in [Open, Fixed, Verified, Deferred, Rejected] {
            assert!(!can_transition(state, state), "{state:?} -> 自分自身");
        }
    }

    /// 409 は理由を本文に入れる（共通の `conflict` だけでは、CLI から使う
    /// レビュワーが「なぜ通らないのか」を判断できない）。
    #[test]
    fn conflicts_carry_the_reason_in_the_body() {
        use common::error::AppError;

        let message = |err: ReviewError| match AppError::from(err) {
            AppError::ConflictDetail(message) => message,
            other => panic!("409 の詳細になっていない: {other:?}"),
        };

        let deferring_high = message(ReviewError::NotDeferrable(FindingSeverity::High));
        assert!(
            deferring_high.contains("high") && deferring_high.contains("繰り延べ"),
            "何が繰り延べられないのか分かる: {deferring_high}"
        );

        let skipping_fixed = message(ReviewError::InvalidTransition {
            from: Open,
            to: Verified,
        });
        assert!(
            skipping_fixed.contains("open") && skipping_fixed.contains("verified"),
            "どの遷移が通らないのか分かる: {skipping_fixed}"
        );

        // 権限の問題は 403 のまま（本文で理由を出し分けない）
        assert!(matches!(
            AppError::from(ReviewError::SelfVerification),
            AppError::Forbidden
        ));
    }

    /// 繰り延べはマージ可否の集計から外れるため、マージ前必須の重大度には許さない。
    #[test]
    fn only_low_and_nit_can_be_deferred() {
        for severity in [FindingSeverity::High, FindingSeverity::Medium] {
            assert!(
                !severity.can_defer(),
                "{severity:?} は繰り延べられない（マージ基準を迂回できてしまう）"
            );
        }
        for severity in [FindingSeverity::Low, FindingSeverity::Nit] {
            assert!(severity.can_defer(), "{severity:?} は繰り延べられる");
        }
        // 繰り延べを許す重大度は、マージを塞がない重大度と一致する
        for severity in [
            FindingSeverity::High,
            FindingSeverity::Medium,
            FindingSeverity::Low,
            FindingSeverity::Nit,
        ] {
            assert_eq!(severity.can_defer(), !severity.blocks_merge());
        }
    }

    #[test]
    fn reviewer_only_transitions_are_the_verification_side() {
        assert!(requires_reviewer_side(Fixed, Verified));
        assert!(requires_reviewer_side(Fixed, Open));
        assert!(requires_reviewer_side(Open, Rejected));
        assert!(requires_reviewer_side(Rejected, Open));
        // 修正の宣言と繰り延べの出入りは修正側も行える
        assert!(!requires_reviewer_side(Open, Fixed));
        assert!(!requires_reviewer_side(Open, Deferred));
        assert!(!requires_reviewer_side(Deferred, Open));
    }

    #[test]
    fn blocking_counts_only_unresolved_high_and_medium() {
        let counts = vec![
            (FindingSeverity::High, Open, 1),
            (FindingSeverity::Medium, Fixed, 2),
            // 確認済み・繰り延べ・棄却はマージを塞がない
            (FindingSeverity::High, Verified, 5),
            (FindingSeverity::Medium, Rejected, 7),
            // Low / Nit は状態にかかわらず塞がない
            (FindingSeverity::Low, Open, 11),
            (FindingSeverity::Nit, Fixed, 13),
        ];
        assert_eq!(blocking_count(&counts), 3);
    }
}
