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
use entity::{
    github_integrations, project_statuses, projects, review_findings, reviews, tasks,
    tenant_members, tenants,
};

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
    /// 指摘を出した本人だけが行える遷移を、別の利用者が行おうとした。
    #[error("transition requires the author of the finding's round")]
    FindingAuthorOnly,
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
            ReviewError::ReviewerOnly
            | ReviewError::FindingAuthorOnly
            | ReviewError::SelfVerification => Self::Forbidden,
            // 既定ステータスが無いプロジェクトでは繰り延べ先タスクを作れない。
            // 利用者が直せる状態の問題なので 409（指摘の状態は変えない）
            ReviewError::NoDefaultStatus(_) => Self::ConflictDetail(
                "プロジェクトに既定ステータスが無いため、繰り延べ先のタスクを作れません".into(),
            ),
            ReviewError::Db(err) => err.into(),
        }
    }
}

/// ラウンドが見ていたリポジトリ。
///
/// プロジェクトの連携先は解除・再連携で差し替えられるので、PR を指すキーには
/// リポジトリを含める。含めないと旧リポジトリの PR #10 と新リポジトリの PR #10 が
/// 同じ PR として続き、旧リポジトリ向けの指摘を新リポジトリへ投稿してしまう（仕様 §3）。
///
/// 連携が無いプロジェクトでは空。`NULL` ではなく空文字にするのは、`UNIQUE` が
/// NULL 同士を別物として扱い、採番の防波堤にならないため。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRef {
    pub integration_id: Option<Uuid>,
    pub owner: String,
    pub name: String,
}

impl RepoRef {
    /// GitHub 連携の無いプロジェクトのラウンド。
    pub fn unlinked() -> Self {
        Self {
            integration_id: None,
            owner: String::new(),
            name: String::new(),
        }
    }

    /// GitHub へ投稿する先を持つか。
    pub fn is_linked(&self) -> bool {
        !self.owner.is_empty() && !self.name.is_empty()
    }

    /// そのラウンドが見ていたリポジトリ。現在の連携先とは限らない。
    pub fn of_round(review: &reviews::Model) -> Self {
        Self {
            integration_id: review.integration_id,
            owner: review.repo_owner.clone(),
            name: review.repo_name.clone(),
        }
    }
}

/// プロジェクトの現在の連携先。連携が無ければ [`RepoRef::unlinked`]。
pub async fn current_repo<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
) -> Result<RepoRef, sea_orm::DbErr> {
    let integration = github_integrations::Entity::find()
        .filter(github_integrations::Column::ProjectId.eq(project_id))
        .one(db)
        .await?;
    Ok(integration
        .map(|row| RepoRef {
            integration_id: Some(row.id),
            owner: row.repo_owner,
            name: row.repo_name,
        })
        .unwrap_or_else(RepoRef::unlinked))
}

/// ラウンドの検索を (project, リポジトリ, pr) に絞る。
fn scoped_rounds(
    project_id: Uuid,
    repo: &RepoRef,
    pr_number: i32,
) -> sea_orm::Select<reviews::Entity> {
    reviews::Entity::find()
        .filter(reviews::Column::ProjectId.eq(project_id))
        .filter(reviews::Column::RepoOwner.eq(repo.owner.clone()))
        .filter(reviews::Column::RepoName.eq(repo.name.clone()))
        .filter(reviews::Column::PrNumber.eq(pr_number))
}

/// PR 内の次のラウンド番号を返す。
///
/// 同じ PR に同時にラウンドを作られると `UNIQUE (project_id, pr_number, round)` に
/// ぶつかるため、プロジェクト行を掴んで採番から挿入までを直列化する。
/// レビューの起票は頻度が低く、この粒度で待たせても実害がない。
pub async fn next_round<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    repo: &RepoRef,
    pr_number: i32,
) -> Result<i32, sea_orm::DbErr> {
    projects::Entity::find_by_id(project_id)
        .lock(LockType::Update)
        .one(db)
        .await?;

    let last: Option<i32> = scoped_rounds(project_id, repo, pr_number)
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
/// 再レビューの判定（解消／未対応）がまさにこの遷移なので、後から出した
/// ラウンドの作成者にも認める。`fixed`（修正の宣言）と `deferred` からの
/// 復帰は修正側も行える。
pub fn requires_reviewer_side(from: FindingState, to: FindingState) -> bool {
    use FindingState::*;
    matches!((from, to), (Fixed, Verified) | (Fixed, Open))
}

/// その遷移が「その指摘を出したラウンドの作成者」に限られるか。
///
/// 取り下げ（`rejected`）だけは [`requires_reviewer_side`] より狭くする。
/// ラウンドは指摘ゼロでも作れるので、「より新しいラウンドの作成者」まで認めると、
/// 修正する側が空のラウンドを 1 本確定するだけでレビュー側を自称でき、他人が
/// 出した High を棄却してマージ基準を 1 人で迂回できてしまう（仕様 §3）。
pub fn requires_finding_author(from: FindingState, to: FindingState) -> bool {
    use FindingState::*;
    matches!((from, to), (Open, Rejected) | (Rejected, Open))
}

/// `actor` が対象 PR のレビュー側か。
///
/// 「その指摘を含むラウンドの作成者」か「同じリポジトリの同じ PR のより新しい
/// ラウンドの作成者」。修正だけを行う利用者を締め出すのが目的で、レビューを
/// 一度でも出した人は以後の確認も行える。
///
/// ラウンド番号はリポジトリごとに 1 から振り直されるので、絞りにリポジトリを
/// 含めないと、旧リポジトリ（あるいは連携前の空リポジトリ）で PR #10 の R3 を
/// 出した人が、新リポジトリの PR #10 でもレビュー側と判定される（仕様 §3）。
pub async fn is_reviewer_side<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    repo: &RepoRef,
    pr_number: i32,
    round: i32,
    actor_id: Uuid,
) -> Result<bool, sea_orm::DbErr> {
    let found = scoped_rounds(project_id, repo, pr_number)
        .filter(reviews::Column::Round.gte(round))
        .filter(reviews::Column::ReviewerId.eq(actor_id))
        .one(db)
        .await?;
    Ok(found.is_some())
}

/// 取り下げをテナントオーナーが代行してよいか。
///
/// 取り下げは本来「その指摘を出したラウンドの作成者だけ」。ただし除名・退会で作成者が
/// テナントの利用者でなくなると、誤った指摘を取り下げる主体が永久に居なくなり、直して
/// いないものを `fixed → verified` と記録するしかなくなる。監査記録に嘘を書かせない
/// ための例外として、この場合に限りオーナーが代行できる（仕様 §3）。
///
/// 作成者が在籍しているうちはオーナーでも代行できない（役割規則を素通しにしない）。
pub async fn may_reject_on_behalf<C: ConnectionTrait>(
    db: &C,
    review: &reviews::Model,
    actor_id: Uuid,
) -> Result<bool, sea_orm::DbErr> {
    let Some(project) = projects::Entity::find_by_id(review.project_id)
        .one(db)
        .await?
    else {
        return Ok(false);
    };
    let Some(tenant) = tenants::Entity::find_by_id(project.tenant_id)
        .one(db)
        .await?
    else {
        return Ok(false);
    };
    if tenant.owner_id != actor_id {
        return Ok(false);
    }
    // オーナー自身が出した指摘なら、そもそも本人として取り下げられる
    if review.reviewer_id == tenant.owner_id {
        return Ok(false);
    }
    let still_a_member = tenant_members::Entity::find()
        .filter(tenant_members::Column::TenantId.eq(tenant.id))
        .filter(tenant_members::Column::UserId.eq(review.reviewer_id))
        .one(db)
        .await?
        .is_some();
    Ok(!still_a_member)
}

/// 遷移の可否を判定する。DB は読むが書かない。
///
/// - 遷移そのものが規則にない → [`ReviewError::InvalidTransition`]
/// - マージ前必須の重大度を繰り延べようとした → [`ReviewError::NotDeferrable`]
/// - レビュー側限定の遷移を修正側が行った → [`ReviewError::ReviewerOnly`]
/// - 取り下げを、指摘を出した本人以外が行った → [`ReviewError::FindingAuthorOnly`]
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

    // 取り下げは、その指摘を出したラウンドの作成者だけ
    // （作成者がテナントから居なくなった場合に限りオーナーが代行できる）
    if requires_finding_author(from, to)
        && review.reviewer_id != actor_id
        && !may_reject_on_behalf(db, review, actor_id).await?
    {
        return Err(ReviewError::FindingAuthorOnly);
    }

    if requires_reviewer_side(from, to)
        && !is_reviewer_side(
            db,
            review.project_id,
            &RepoRef::of_round(review),
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

/// 繰り延べ先タスクが「有効」か——同じプロジェクトにあり、削除されていないか。
///
/// タスクの削除はソフトデリート（`deleted_at`）で `deferred_task_id` の外部キーは
/// 外れないため、リンクが残っているかでは判定できない。リンクだけを見て再オープン
/// すると、利用者が消したタスクを黙って復活させてしまう（仕様 §3）。
async fn find_live_deferred_task<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    task_id: Uuid,
) -> Result<Option<tasks::Model>, sea_orm::DbErr> {
    tasks::Entity::find_by_id(task_id)
        .filter(tasks::Column::ProjectId.eq(project_id))
        .filter(tasks::Column::DeletedAt.is_null())
        .one(db)
        .await
}

/// 畳んであった繰り延べ先タスクを開き直す。
///
/// 繰り延べを往復するたびに起票すると `seq_id` と通知を消費してタスク一覧が同じ内容で
/// 埋まるので、有効なタスクが残っていれば使い回す（仕様 §3）。
async fn reopen_deferred_task<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    task: tasks::Model,
) -> Result<(), ReviewError> {
    let default_status = project_statuses::Entity::find()
        .filter(project_statuses::Column::ProjectId.eq(project_id))
        .filter(project_statuses::Column::IsDefault.eq(true))
        .one(db)
        .await?
        .ok_or(ReviewError::NoDefaultStatus(project_id))?;

    let mut active: tasks::ActiveModel = task.into();
    active.status_id = Set(default_status.id);
    active.completed_at = Set(None);
    active.updated_at = Set(chrono::Utc::now().into());
    active.update(db).await?;
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

    // 繰り延べの出入りで、リンク先タスクを作る／畳む。
    // 不変条件は「常に同じ物理タスク」ではなく「同時に存在する有効なタスクは 1 件」
    let mut deferred_task_id = finding.deferred_task_id;
    if to == FindingState::Deferred {
        // 前回のタスクが残っていれば開き直し、消えていれば代替を 1 件起票する
        let live = match finding.deferred_task_id {
            Some(task_id) => find_live_deferred_task(db, review.project_id, task_id).await?,
            None => None,
        };
        match live {
            Some(task) => {
                let task_id = task.id;
                reopen_deferred_task(db, review.project_id, task).await?;
                deferred_task_id = Some(task_id);
            }
            None => {
                let task =
                    create_deferred_task(db, review.project_id, &finding, review, actor_id).await?;
                deferred_task_id = Some(task.id);
            }
        }
    } else if from == FindingState::Deferred
        && let Some(task_id) = finding.deferred_task_id
    {
        close_deferred_task(db, review.project_id, task_id).await?;
        // リンクは残す。次の繰り延べで開き直せるようにするため
        // （消えていれば代替を起票してリンクを差し替える）
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

/// オーナー代行で棄却された指摘の件数。
///
/// 代行の条件（作成者がテナントから居なくなっていること）はオーナー自身が除名で
/// 作れるので、防ぐ代わりに痕跡を残す（仕様 §2 / §5）。数え方は「`rejected` へ
/// 遷移させたのがその指摘を出したラウンドの作成者以外」——代行できるのは
/// オーナーだけなので、これで代行だけが数えられる。
pub async fn owner_override_rejection_count<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    repo: &RepoRef,
    pr_number: i32,
) -> Result<u64, sea_orm::DbErr> {
    let rounds = scoped_rounds(project_id, repo, pr_number).all(db).await?;
    if rounds.is_empty() {
        return Ok(0);
    }
    let reviewer_by_review: std::collections::HashMap<Uuid, Uuid> =
        rounds.iter().map(|r| (r.id, r.reviewer_id)).collect();

    let rows: Vec<(Uuid, Uuid)> = entity::review_finding_transitions::Entity::find()
        .inner_join(review_findings::Entity)
        .filter(review_findings::Column::ReviewId.is_in(reviewer_by_review.keys().copied()))
        .filter(entity::review_finding_transitions::Column::ToState.eq(FindingState::Rejected))
        .select_only()
        .column(review_findings::Column::ReviewId)
        .column(entity::review_finding_transitions::Column::ActorId)
        .into_tuple()
        .all(db)
        .await?;

    Ok(rows
        .into_iter()
        .filter(|(review_id, actor_id)| {
            reviewer_by_review
                .get(review_id)
                .is_some_and(|reviewer_id| reviewer_id != actor_id)
        })
        .count() as u64)
}

/// PR 単位の集計（重大度 × 状態の件数）。
pub async fn severity_state_counts<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    repo: &RepoRef,
    pr_number: i32,
) -> Result<Vec<(FindingSeverity, FindingState, u64)>, sea_orm::DbErr> {
    let rows: Vec<(FindingSeverity, FindingState, i64)> = review_findings::Entity::find()
        .inner_join(reviews::Entity)
        .filter(reviews::Column::ProjectId.eq(project_id))
        .filter(reviews::Column::RepoOwner.eq(repo.owner.clone()))
        .filter(reviews::Column::RepoName.eq(repo.name.clone()))
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

/// 最新ラウンドがレビューした commit。ラウンドが無ければ `None`。
///
/// 現在の HEAD と突き合わせるのは呼び出し側（CLI）。読み取り API から GitHub を
/// 呼ばないのは、マージ前ゲートの応答時間と可用性を GitHub に握らせないため（仕様 §5）。
pub async fn latest_head_sha<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    repo: &RepoRef,
    pr_number: i32,
) -> Result<Option<String>, sea_orm::DbErr> {
    let latest: Option<String> = scoped_rounds(project_id, repo, pr_number)
        .select_only()
        .column(reviews::Column::HeadSha)
        .order_by_desc(reviews::Column::Round)
        .into_tuple()
        .one(db)
        .await?;
    Ok(latest)
}

/// 要約ジョブが最後に GitHub で確かめた head と、その時刻。
///
/// 画面はこれと最新ラウンドの `head_sha` を比べて「レビューが古い」を出す。
/// push では更新されないので、時刻を添えて「いつ時点の確認か」を示す（仕様 §5 / §8）。
pub async fn cached_pr_head<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    repo: &RepoRef,
    pr_number: i32,
) -> Result<(Option<String>, Option<chrono::DateTime<chrono::Utc>>), sea_orm::DbErr> {
    let row: Option<(
        Option<String>,
        Option<chrono::DateTime<chrono::FixedOffset>>,
    )> = scoped_rounds(project_id, repo, pr_number)
        .select_only()
        .column(reviews::Column::PrHeadSha)
        .column(reviews::Column::PrHeadCheckedAt)
        .order_by_desc(reviews::Column::Round)
        .into_tuple()
        .one(db)
        .await?;
    Ok(match row {
        Some((sha, at)) => (sha, at.map(|t| t.with_timezone(&chrono::Utc))),
        None => (None, None),
    })
}

/// PR 内のラウンド数（= 最大の round）。
pub async fn round_count<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    repo: &RepoRef,
    pr_number: i32,
) -> Result<i32, sea_orm::DbErr> {
    let last: Option<i32> = scoped_rounds(project_id, repo, pr_number)
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
        // 修正の宣言と繰り延べの出入りは修正側も行える
        assert!(!requires_reviewer_side(Open, Fixed));
        assert!(!requires_reviewer_side(Open, Deferred));
        assert!(!requires_reviewer_side(Deferred, Open));
    }

    /// 取り下げだけは「レビュー側」より狭く、指摘を出した本人に限る。
    ///
    /// ラウンドは指摘ゼロでも作れるので、より新しいラウンドの作成者まで認めると、
    /// 空のラウンドを 1 本作るだけで他人の High を棄却でき、マージ基準を
    /// 1 人で迂回できてしまう。
    #[test]
    fn rejecting_is_limited_to_the_author_of_the_finding() {
        assert!(requires_finding_author(Open, Rejected));
        assert!(requires_finding_author(Rejected, Open));
        // 取り下げは「レビュー側」の緩い方には載せない（二重判定にしない）
        assert!(!requires_reviewer_side(Open, Rejected));
        assert!(!requires_reviewer_side(Rejected, Open));
        // 確認と差し戻しは後続ラウンドの作成者にも許す（再レビューの判定そのもの）
        assert!(!requires_finding_author(Fixed, Verified));
        assert!(!requires_finding_author(Fixed, Open));
        // 修正側が行える遷移は、どちらの制約にも載らない
        for (from, to) in [(Open, Fixed), (Open, Deferred), (Deferred, Open)] {
            assert!(!requires_finding_author(from, to));
            assert!(!requires_reviewer_side(from, to));
        }
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
