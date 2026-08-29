use std::collections::HashMap;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use axum_valid::Valid;
use sea_orm::sea_query::LockType;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, TransactionTrait, prelude::Uuid,
};
use serde::Deserialize;
use utoipa::IntoParams;

use crate::AppState;
use crate::error::{AppError, ServerError};
use crate::extractors::AuthUser;
use crate::openapi::CrudErrors;
use entity::review_findings::{FindingSeverity, FindingState};
use entity::{review_finding_transitions, review_findings, reviews, scopes::Scope, users};
use payload::reviews::*;
#[derive(Debug, Deserialize, IntoParams)]
pub struct PrQuery {
    /// 対象 PR 番号
    pub pr: i32,
    /// 見るリポジトリ（`owner/name`）。既定は現在の連携先
    ///
    /// 連携を差し替える前のラウンドや、連携を張る前に溜めたラウンド（空文字で指定）を
    /// 読むために使う。無いと「履歴として残る」と言いながら読む手段が無い（仕様 §5）。
    pub repo: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct FindingListQuery {
    /// 対象 PR 番号
    pub pr: i32,
    /// 見るリポジトリ（`owner/name`）。既定は現在の連携先（`PrQuery::repo` と同じ）
    pub repo: Option<String>,
    /// 状態での絞り込み（カンマ区切り。例: `open,fixed`）
    pub state: Option<String>,
    /// 重大度での絞り込み（カンマ区切り。例: `high,medium`）
    pub severity: Option<String>,
}

/// 読み取りの視界を決める。`repo` の指定があればそれ、無ければ現在の連携先。
///
/// 指定は `owner/name`。空文字は「連携が無かった頃のラウンド」を指す。
/// 形式が違えば 400（黙って現在の連携先へ落とすと、読めていないことに気づけない）。
async fn resolve_repo_scope(
    state: &AppState,
    project_id: Uuid,
    repo: Option<&str>,
) -> Result<service::reviews::RepoRef, AppError> {
    let Some(raw) = repo else {
        return Ok(service::reviews::current_repo(&state.db, project_id).await?);
    };
    if raw.is_empty() {
        return Ok(service::reviews::RepoRef::unlinked());
    }
    let Some((owner, name)) = raw.split_once('/') else {
        return Err(AppError::BadRequestDetail(format!(
            "repo は owner/name の形で指定してください（受け取った値: {raw}）"
        )));
    };
    if owner.is_empty() || name.is_empty() || name.contains('/') {
        return Err(AppError::BadRequestDetail(format!(
            "repo は owner/name の形で指定してください（受け取った値: {raw}）"
        )));
    }
    Ok(service::reviews::RepoRef {
        // 過去の連携の行は残っていないことがあるので、控えの文字列だけで絞る
        integration_id: None,
        owner: owner.to_string(),
        name: name.to_string(),
    })
}

/// カンマ区切りのクエリを列挙値へ。未知の値が混ざっていたら 400 にする
/// （綴り違いを黙って無視すると、絞り込みが効いていないことに気づけない）。
///
/// 400 には対象のパラメーター名と受け取った値を入れる。素の `bad request` だけでは、
/// CLI から使うレビュワー（AI を含む）がどの値の綴り違いなのかを判断できない。
fn parse_csv<T: std::str::FromStr>(
    param: &str,
    raw: Option<&str>,
) -> Result<Option<Vec<T>>, AppError> {
    let Some(raw) = raw else { return Ok(None) };
    let mut values = Vec::new();
    for part in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        values.push(part.parse::<T>().map_err(|_| {
            AppError::BadRequestDetail(format!(
                "{param} に未知の値があります（受け取った値: {part}）"
            ))
        })?);
    }
    Ok((!values.is_empty()).then_some(values))
}

/// テナント配下のプロジェクトであることを確かめたうえで、レビューを読む権限を見る。
async fn ensure_read_access(
    state: &AppState,
    auth: &AuthUser,
    tenant_id: Uuid,
    project_id: Uuid,
) -> Result<(), AppError> {
    auth.require_scope(Scope::ReadReview)?;
    auth.ensure_tenant_access(state, tenant_id, Some(project_id))
        .await
}

async fn ensure_write_access(
    state: &AppState,
    auth: &AuthUser,
    tenant_id: Uuid,
    project_id: Uuid,
) -> Result<(), AppError> {
    auth.require_scope(Scope::WriteReview)?;
    auth.ensure_tenant_access(state, tenant_id, Some(project_id))
        .await
}

/// 指摘に紐づく遷移履歴を、実行者つきでまとめて引く。
async fn load_transitions<C: ConnectionTrait>(
    db: &C,
    finding_ids: &[Uuid],
) -> Result<HashMap<Uuid, Vec<FindingTransitionResponse>>, AppError> {
    if finding_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let rows = review_finding_transitions::Entity::find()
        .filter(review_finding_transitions::Column::FindingId.is_in(finding_ids.to_vec()))
        .order_by_asc(review_finding_transitions::Column::CreatedAt)
        .order_by_asc(review_finding_transitions::Column::Id)
        .all(db)
        .await?;

    let actor_ids: Vec<Uuid> = rows.iter().map(|row| row.actor_id).collect();
    let actors = load_users(db, &actor_ids).await?;

    let mut by_finding: HashMap<Uuid, Vec<FindingTransitionResponse>> = HashMap::new();
    for row in rows {
        let actor = actors
            .get(&row.actor_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("transition actor {} has no user row", row.actor_id))?;
        by_finding
            .entry(row.finding_id)
            .or_default()
            .push(FindingTransitionResponse::from_parts(row, actor));
    }
    Ok(by_finding)
}

/// 利用者行をまとめて引く。FK があるため全件そろう前提で、欠けたら握り潰さず 500 にする。
async fn load_users<C: ConnectionTrait>(
    db: &C,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, users::Model>, AppError> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = users::Entity::find()
        .filter(users::Column::Id.is_in(ids.to_vec()))
        .all(db)
        .await?;
    Ok(rows.into_iter().map(|user| (user.id, user)).collect())
}

async fn find_review<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    review_id: Uuid,
) -> Result<reviews::Model, AppError> {
    reviews::Entity::find_by_id(review_id)
        .filter(reviews::Column::ProjectId.eq(project_id))
        .one(db)
        .await?
        .ok_or(AppError::NotFound)
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/",
    operation_id = "create_review",
    tag = "Reviews",
    summary = "レビューラウンドを起票（指摘の一括作成）",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
    ),
    request_body = CreateReviewRequest,
    responses(
        (status = 201, description = "作成されたラウンドと指摘", body = ReviewDetailResponse),
        (status = 409, description = "同時起票でラウンド番号が衝突しました", body = ServerError),
        CrudErrors,
    )
)]
pub async fn create_review(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id)): Path<(Uuid, Uuid)>,
    Valid(Json(payload)): Valid<Json<CreateReviewRequest>>,
) -> Result<(StatusCode, Json<ReviewDetailResponse>), AppError> {
    ensure_write_access(&state, &auth, tenant_id, project_id).await?;

    let txn = state.db.begin().await?;

    // ラウンドは確定時に指摘ごと作る。確定後の追記は API として提供しない
    // （「どの head を見た時点の判断か」を濁さないため。仕様 §3）
    // どのリポジトリの PR を見たかを控える。連携先は差し替えられるので、
    // これが無いと別リポジトリの同番 PR が同じ PR として続く（仕様 §3）
    let repo = service::reviews::current_repo(&txn, project_id).await?;
    let round = service::reviews::next_round(&txn, project_id, &repo, payload.pr_number).await?;
    let now = chrono::Utc::now();

    let review = reviews::ActiveModel {
        id: Set(Uuid::new_v4()),
        project_id: Set(project_id),
        integration_id: Set(repo.integration_id),
        repo_owner: Set(repo.owner.clone()),
        repo_name: Set(repo.name.clone()),
        pr_number: Set(payload.pr_number),
        round: Set(round),
        head_sha: Set(payload.head_sha),
        reviewer_id: Set(auth.user_id),
        summary: Set(payload.summary),
        pr_title: Set(None),
        pr_author: Set(None),
        // 要約ジョブが投稿時に埋める（鮮度の確認とコメントの控え）
        pr_head_sha: Set(None),
        pr_head_checked_at: Set(None),
        summary_comment_id: Set(None),
        created_at: Set(now.into()),
    }
    .insert(&txn)
    .await?;

    let mut findings = Vec::with_capacity(payload.findings.len());
    for input in payload.findings {
        let finding = review_findings::ActiveModel {
            id: Set(Uuid::new_v4()),
            review_id: Set(review.id),
            severity: Set(input.severity),
            title: Set(input.title),
            body: Set(input.body),
            file: Set(input.file),
            line: Set(input.line),
            state: Set(FindingState::Open),
            deferred_task_id: Set(None),
            fixed_by: Set(None),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
        }
        .insert(&txn)
        .await?;

        // 起票も履歴に残す（from が NULL の行）
        service::reviews::record_transition(
            &txn,
            finding.id,
            auth.user_id,
            None,
            FindingState::Open,
            Some(format!("{} を見て指摘", review.head_sha)),
        )
        .await?;
        findings.push(finding);
    }

    let reviewer = users::Entity::find_by_id(auth.user_id)
        .one(&txn)
        .await?
        .ok_or_else(|| anyhow::anyhow!("reviewer {} has no user row", auth.user_id))?;

    txn.commit().await?;

    let finding_ids: Vec<Uuid> = findings.iter().map(|f| f.id).collect();
    let mut transitions = load_transitions(&state.db, &finding_ids).await?;
    let pr_number = review.pr_number;
    let round = review.round;
    let count = findings.len() as u64;

    let detail = ReviewDetailResponse {
        review: ReviewResponse::from_parts(review, reviewer, count),
        findings: findings
            .into_iter()
            .map(|finding| {
                let history = transitions.remove(&finding.id).unwrap_or_default();
                FindingResponse::from_parts(finding, pr_number, round, history)
            })
            .collect(),
    };

    Ok((StatusCode::CREATED, Json(detail)))
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/",
    operation_id = "list_reviews",
    tag = "Reviews",
    summary = "PR のレビューラウンド一覧",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        PrQuery,
    ),
    responses(
        (status = 200, description = "ラウンド一覧（新しい順）", body = [ReviewResponse]),
        CrudErrors,
    )
)]
pub async fn list_reviews(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<PrQuery>,
) -> Result<Json<Vec<ReviewResponse>>, AppError> {
    ensure_read_access(&state, &auth, tenant_id, project_id).await?;

    // 一覧が見るのは既定で現在の連携先のラウンドだけ。連携を差し替えた後に旧リポジトリの
    // 同番 PR のラウンドが混ざらないようにする（仕様 §3。`repo` で明示もできる）
    let repo = resolve_repo_scope(&state, project_id, query.repo.as_deref()).await?;
    let rounds = reviews::Entity::find()
        .filter(reviews::Column::ProjectId.eq(project_id))
        .filter(reviews::Column::RepoOwner.eq(repo.owner.clone()))
        .filter(reviews::Column::RepoName.eq(repo.name.clone()))
        .filter(reviews::Column::PrNumber.eq(query.pr))
        .order_by_desc(reviews::Column::Round)
        .all(&state.db)
        .await?;

    let reviewer_ids: Vec<Uuid> = rounds.iter().map(|r| r.reviewer_id).collect();
    let reviewers = load_users(&state.db, &reviewer_ids).await?;

    let mut out = Vec::with_capacity(rounds.len());
    for round in rounds {
        let count = review_findings::Entity::find()
            .filter(review_findings::Column::ReviewId.eq(round.id))
            .count(&state.db)
            .await?;
        let reviewer = reviewers
            .get(&round.reviewer_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("reviewer {} has no user row", round.reviewer_id))?;
        out.push(ReviewResponse::from_parts(round, reviewer, count));
    }
    Ok(Json(out))
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/summary",
    operation_id = "get_review_summary",
    tag = "Reviews",
    summary = "PR 単位の集計（マージ可否）",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        PrQuery,
    ),
    responses(
        (status = 200, description = "重大度 × 状態の件数とマージ可否", body = ReviewSummaryResponse),
        CrudErrors,
    )
)]
pub async fn get_review_summary(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<PrQuery>,
) -> Result<Json<ReviewSummaryResponse>, AppError> {
    ensure_read_access(&state, &auth, tenant_id, project_id).await?;

    let repo = resolve_repo_scope(&state, project_id, query.repo.as_deref()).await?;
    let counts =
        service::reviews::severity_state_counts(&state.db, project_id, &repo, query.pr).await?;
    let blocking = service::reviews::blocking_count(&counts);
    let rounds = service::reviews::round_count(&state.db, project_id, &repo, query.pr).await?;
    let latest_head_sha =
        service::reviews::latest_head_sha(&state.db, project_id, &repo, query.pr).await?;
    let owner_override_rejections =
        service::reviews::owner_override_rejection_count(&state.db, project_id, &repo, query.pr)
            .await?;
    let (cached_pr_head_sha, pr_head_checked_at) =
        service::reviews::cached_pr_head(&state.db, project_id, &repo, query.pr).await?;

    Ok(Json(ReviewSummaryResponse {
        pr_number: query.pr,
        rounds,
        counts: counts
            .into_iter()
            .map(|(severity, state, count)| SeverityStateCount {
                severity,
                state,
                count,
            })
            .collect(),
        blocking,
        latest_head_sha,
        cached_pr_head_sha,
        pr_head_checked_at,
        owner_override_rejections,
        repository: repo
            .is_linked()
            .then(|| format!("{}/{}", repo.owner, repo.name)),
        // レビューが 1 件も無い PR を「可」にしない。件数だけで見ると未レビューの PR が
        // 0 件として通り、マージ前ゲートとして最も危ない誤りになる（仕様 §5）
        mergeable: rounds > 0 && blocking == 0,
    }))
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/{id}",
    operation_id = "get_review",
    tag = "Reviews",
    summary = "レビューラウンドの詳細（指摘つき）",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = Uuid, Path, description = "ラウンドID"),
    ),
    responses(
        (status = 200, description = "ラウンドと指摘", body = ReviewDetailResponse),
        CrudErrors,
    )
)]
pub async fn get_review(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, review_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<ReviewDetailResponse>, AppError> {
    ensure_read_access(&state, &auth, tenant_id, project_id).await?;

    let review = find_review(&state.db, project_id, review_id).await?;
    let findings = review_findings::Entity::find()
        .filter(review_findings::Column::ReviewId.eq(review.id))
        .order_by_asc(review_findings::Column::CreatedAt)
        .order_by_asc(review_findings::Column::Id)
        .all(&state.db)
        .await?;

    let reviewer = users::Entity::find_by_id(review.reviewer_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("reviewer {} has no user row", review.reviewer_id))?;

    let finding_ids: Vec<Uuid> = findings.iter().map(|f| f.id).collect();
    let mut transitions = load_transitions(&state.db, &finding_ids).await?;
    let pr_number = review.pr_number;
    let round = review.round;
    let count = findings.len() as u64;

    Ok(Json(ReviewDetailResponse {
        review: ReviewResponse::from_parts(review, reviewer, count),
        findings: findings
            .into_iter()
            .map(|finding| {
                let history = transitions.remove(&finding.id).unwrap_or_default();
                FindingResponse::from_parts(finding, pr_number, round, history)
            })
            .collect(),
    }))
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/",
    operation_id = "list_review_findings",
    tag = "Reviews",
    summary = "PR の指摘一覧（状態・重大度で絞り込み）",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        FindingListQuery,
    ),
    responses(
        (status = 200, description = "指摘一覧（ラウンド順）", body = [FindingResponse]),
        CrudErrors,
    )
)]
pub async fn list_review_findings(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<FindingListQuery>,
) -> Result<Json<Vec<FindingResponse>>, AppError> {
    ensure_read_access(&state, &auth, tenant_id, project_id).await?;

    let states = parse_csv::<FindingState>("state", query.state.as_deref())?;
    let severities = parse_csv::<FindingSeverity>("severity", query.severity.as_deref())?;

    let repo = resolve_repo_scope(&state, project_id, query.repo.as_deref()).await?;
    let rounds = reviews::Entity::find()
        .filter(reviews::Column::ProjectId.eq(project_id))
        .filter(reviews::Column::RepoOwner.eq(repo.owner.clone()))
        .filter(reviews::Column::RepoName.eq(repo.name.clone()))
        .filter(reviews::Column::PrNumber.eq(query.pr))
        .order_by_asc(reviews::Column::Round)
        .all(&state.db)
        .await?;
    let round_by_id: HashMap<Uuid, (i32, i32)> = rounds
        .iter()
        .map(|r| (r.id, (r.pr_number, r.round)))
        .collect();
    if round_by_id.is_empty() {
        return Ok(Json(Vec::new()));
    }

    let mut find = review_findings::Entity::find().filter(
        review_findings::Column::ReviewId.is_in(round_by_id.keys().copied().collect::<Vec<_>>()),
    );
    if let Some(states) = states {
        find = find.filter(review_findings::Column::State.is_in(states));
    }
    if let Some(severities) = severities {
        find = find.filter(review_findings::Column::Severity.is_in(severities));
    }
    let findings = find
        .order_by_asc(review_findings::Column::CreatedAt)
        .order_by_asc(review_findings::Column::Id)
        .all(&state.db)
        .await?;

    let finding_ids: Vec<Uuid> = findings.iter().map(|f| f.id).collect();
    let mut transitions = load_transitions(&state.db, &finding_ids).await?;

    Ok(Json(
        findings
            .into_iter()
            .map(|finding| {
                let (pr_number, round) = round_by_id
                    .get(&finding.review_id)
                    .copied()
                    .unwrap_or((query.pr, 0));
                let history = transitions.remove(&finding.id).unwrap_or_default();
                FindingResponse::from_parts(finding, pr_number, round, history)
            })
            .collect(),
    ))
}

#[axum::debug_handler]
#[utoipa::path(
    patch,
    path = "/{id}",
    operation_id = "update_review_finding_state",
    tag = "Reviews",
    summary = "指摘の状態を進める",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = Uuid, Path, description = "指摘ID"),
    ),
    request_body = UpdateFindingStateRequest,
    responses(
        (status = 200, description = "更新後の指摘", body = FindingResponse),
        (status = 403, description = "レビュー側限定の遷移、または自分の修正の確認", body = ServerError),
        (status = 409, description = "現在の状態からは行えない遷移、または High / Medium の繰り延べ", body = ServerError),
        CrudErrors,
    )
)]
pub async fn update_review_finding_state(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, finding_id)): Path<(Uuid, Uuid, Uuid)>,
    Valid(Json(payload)): Valid<Json<UpdateFindingStateRequest>>,
) -> Result<Json<FindingResponse>, AppError> {
    ensure_write_access(&state, &auth, tenant_id, project_id).await?;

    let txn = state.db.begin().await?;

    // 指摘の行を掴んでから状態を読む。掴まないと、同じ open の指摘へ deferred が
    // 同時に届いたとき双方が繰り延べ先タスクを起票し、後勝ちのリンクだけが残って
    // もう 1 件が参照されない孤児になる（仕様 §3）
    let finding = review_findings::Entity::find_by_id(finding_id)
        .lock(LockType::Update)
        .one(&txn)
        .await?
        .ok_or(AppError::NotFound)?;
    // 他プロジェクトの指摘 ID を渡されても存在を漏らさない
    let review = find_review(&txn, project_id, finding.review_id).await?;

    let updated = service::reviews::apply_transition(
        &txn,
        finding,
        &review,
        payload.state,
        auth.user_id,
        payload.note,
    )
    .await?;

    txn.commit().await?;

    let transitions = load_transitions(&state.db, &[updated.id])
        .await?
        .remove(&updated.id)
        .unwrap_or_default();

    Ok(Json(FindingResponse::from_parts(
        updated,
        review.pr_number,
        review.round,
        transitions,
    )))
}
