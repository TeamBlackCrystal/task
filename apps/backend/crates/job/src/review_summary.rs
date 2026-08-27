//! レビュー指摘の要約を PR コメントへ反映するジョブ。
//!
//! ラウンドの起票時と指摘の状態遷移時に投入される。GitHub には
//! マーカー付きのコメント 1 本だけを置き、以後は同じコメントを編集する
//! （仕様 `docs/features/review-findings.md` §7）。
//!
//! 失敗はベストエフォート: 投稿・編集に失敗しても API 側の起票・遷移は
//! 巻き戻さない。GitHub 連携の無いプロジェクトでは何もしない。
//!
//! 同一 (project, pr) の更新要求は 1 本に合流させる（`service::github::review_summary_queue`）。
//! 遷移のたびに積むと同じコメントへ連続して書き込み、GitHub の
//! secondary rate limit に当たるため。実行区間も同じ単位でロックして直列化する
//! （並行して走ると、古い状態を読んだ側の書き込みが後から着いてコメントが巻き戻る）。

use std::sync::Arc;
use std::time::Duration;

use apalis::prelude::{
    BackoffConfig, BoxDynError, Data, IntervalStrategy, StrategyBuilder, TaskSink,
};
use apalis_postgres::{Config, JsonCodec, PgPool, PostgresStorage};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use common::cache::redis::RedisConnection;
use common::settings::Settings;
use entity::github_integrations;

use crate::JobState;

pub const QUEUE_NAME: &str = "review_summary";
pub const MAX_RETRIES: usize = 3;

/// キューを見に行く間隔。「更新待ち」フラグの TTL は、この刻みで待たされる時間より
/// 十分に長くないと、ジョブが走り出す前にフラグが切れて合流が効かなくなる。
pub const POLL_INTERVAL_SECS: u64 = 2;

/// 「更新待ち」フラグの TTL が、キューでの滞留に対して桁で余裕があること。
///
/// ここが短いと、ジョブが走り出してフラグを落とす前に期限が切れ、以後の遷移が
/// それぞれ別のジョブを積む。合流が効かなくなって同じコメントへ連続して書きに
/// 行くが、失敗はしないので外からは気づけない（仕様 §7）。どちらかの値を
/// 動かしたらビルドで止める。
const _: () = assert!(
    service::github::review_summary_queue::SUMMARY_PENDING_TTL_SECS >= POLL_INTERVAL_SECS * 30,
    "更新待ちフラグの TTL がキューの滞留に対して短すぎる"
);

/// 更新対象の PR。ペイロードは ID と番号だけで、トークン等は載せない
/// （apalis のジョブは Postgres に平文で永続化される）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewSummaryJob {
    pub project_id: Uuid,
    pub pr_number: i32,
}

pub type ReviewSummaryStorage = PostgresStorage<
    ReviewSummaryJob,
    apalis_postgres::CompactType,
    JsonCodec<apalis_postgres::CompactType>,
    apalis_postgres::PgNotify,
>;

pub fn build_storage(pool: &PgPool, _settings: &Settings) -> ReviewSummaryStorage {
    let config = Config::new(QUEUE_NAME).with_poll_interval(
        StrategyBuilder::new()
            .apply(
                IntervalStrategy::new(Duration::from_secs(POLL_INTERVAL_SECS))
                    .with_backoff(BackoffConfig::default()),
            )
            .build(),
    );
    PostgresStorage::new_with_notify(pool, &config)
}

pub async fn setup(
    pool: &PgPool,
    settings: &Settings,
) -> Result<Arc<ReviewSummaryStorage>, anyhow::Error> {
    PostgresStorage::setup(pool).await?;
    Ok(Arc::new(build_storage(pool, settings)))
}

pub async fn enqueue(
    storage: &ReviewSummaryStorage,
    job: ReviewSummaryJob,
) -> Result<(), anyhow::Error> {
    let mut storage = storage.clone();
    storage
        .push(job)
        .await
        .map_err(|e| anyhow::anyhow!("push review summary job: {e}"))?;
    Ok(())
}

/// 更新待ちのジョブが無いときだけ投入する。
///
/// 投稿に失敗してもジョブ側の呼び出し元（API）を巻き込まないよう、
/// enqueue の失敗は警告に留める。合流の判定に使う Redis が落ちている場合も
/// 同じで、そのときは合流せずに積む（要約が止まるより、多めに書きに行く方がよい）。
pub async fn enqueue_best_effort(
    storage: &ReviewSummaryStorage,
    redis: &RedisConnection,
    project_id: Uuid,
    repo: &str,
    pr_number: i32,
) {
    match service::github::review_summary_queue::try_mark_pending(
        redis, project_id, repo, pr_number,
    )
    .await
    {
        Ok(false) => {
            // 既に積まれているジョブが、実行時に最新状態を読み直して反映する
            tracing::debug!(%project_id, pr_number, "review summary update is already pending");
            return;
        }
        Ok(true) => {}
        Err(e) => {
            tracing::warn!(error = %e, %project_id, pr_number, "review summary pending flag failed");
        }
    }

    if let Err(e) = enqueue(
        storage,
        ReviewSummaryJob {
            project_id,
            pr_number,
        },
    )
    .await
    {
        tracing::warn!(error = %e, %project_id, pr_number, "enqueue review summary failed");
        // 積めなかったフラグを残すと、TTL のあいだ以降の更新まで合流で捨てられる
        if let Err(e) =
            service::github::review_summary_queue::clear_pending(redis, project_id, repo, pr_number)
                .await
        {
            tracing::warn!(error = %e, %project_id, pr_number, "clear review summary pending failed");
        }
    }
}

pub async fn process(job: ReviewSummaryJob, state: Data<JobState>) -> Result<(), BoxDynError> {
    // 投稿先も鍵の単位も「現在の連携先リポジトリ」で決める。ラウンドはどのリポジトリの
    // PR を見たかを控えているので、連携を差し替えた後は旧リポジトリのラウンドが
    // この範囲から外れ、旧リポジトリ向けの指摘を新リポジトリへ書き込むことがない（仕様 §7）
    let repo = service::reviews::current_repo(&state.db, job.project_id).await?;
    let repo_key = format!("{}/{}", repo.owner, repo.name);

    // 同じ PR を更新中のジョブがいる間は投稿しない。並行して走ると、先に古い
    // 状態を読んだ側の書き込みが後から着き、コメントが巻き戻ったまま次の遷移まで
    // 直らない
    let Some(token) = service::github::review_summary_queue::try_acquire_update_lock(
        &state.redis_client,
        job.project_id,
        &repo_key,
        job.pr_number,
    )
    .await?
    else {
        // 「更新待ち」の印は落とさない。この再試行が拾い直し、そのとき最新状態を読む
        tracing::info!(
            project_id = %job.project_id,
            pr = job.pr_number,
            "another review summary update is running; retrying later"
        );
        return Err("review summary update is already running".into());
    };

    let result = update_summary(&job, &state, &repo, &repo_key).await;

    if let Err(e) = service::github::review_summary_queue::release_update_lock(
        &state.redis_client,
        job.project_id,
        &repo_key,
        job.pr_number,
        &token,
    )
    .await
    {
        // 残すと TTL のあいだ後続が全部再試行に回る
        tracing::warn!(error = %e, project_id = %job.project_id, pr = job.pr_number, "release review summary lock failed");
    }

    result
}

/// ロックを取った状態で、最新の集計を読んでコメントへ反映する。
async fn update_summary(
    job: &ReviewSummaryJob,
    state: &JobState,
    repo: &service::reviews::RepoRef,
    repo_key: &str,
) -> Result<(), BoxDynError> {
    // 状態を読む前に落とす。順序を逆にすると、読んだ後・落とす前の遷移が
    // 合流で捨てられて要約に出ない。先に落として取りこぼす側は
    // ジョブが 1 本余計に積まれるだけで済む
    if let Err(e) = service::github::review_summary_queue::clear_pending(
        &state.redis_client,
        job.project_id,
        repo_key,
        job.pr_number,
    )
    .await
    {
        tracing::warn!(error = %e, project_id = %job.project_id, pr = job.pr_number, "clear review summary pending failed");
    }

    let Some(github) = state.settings.github_app.as_ref() else {
        tracing::warn!("github app is not configured; skipping review summary");
        return Ok(());
    };

    // 連携の無いプロジェクトでは投稿しない（起票・管理自体は task 側で完結する）
    let Some(integration) = github_integrations::Entity::find()
        .filter(github_integrations::Column::ProjectId.eq(job.project_id))
        .one(&state.db)
        .await?
    else {
        tracing::debug!(project_id = %job.project_id, "no github integration; skipping review summary");
        return Ok(());
    };

    let token = service::github::installation_token(
        &state.http_client,
        github,
        integration.installation_id,
    )
    .await?;

    // 表示用の PR メタは取れたときだけ更新する。取れなくても要約は出す
    // （PR 番号だけで用は足りるので、ここで止めると本題を落とす）。
    // 同じ応答に現在の head が入っているので、鮮度の照合にも使う（仕様 §7）
    let current_head_sha = match service::github::pr_comments::fetch_pull_request(
        &state.http_client,
        &token,
        &integration.repo_owner,
        &integration.repo_name,
        job.pr_number,
    )
    .await
    {
        Ok(meta) => {
            let head = meta.head.as_ref().map(|h| h.sha.clone());
            service::reviews::cache_pr_meta(
                &state.db,
                job.project_id,
                repo,
                job.pr_number,
                &meta.title,
                meta.user.as_ref().map(|u| u.login.as_str()),
                head.as_deref(),
            )
            .await?;
            head
        }
        Err(e) => {
            // 取れなければ鮮度を確かめられない。本文は「鮮度不明」になり、
            // マージ可は出さない（仕様 §7）
            tracing::warn!(error = %e, pr = job.pr_number, "fetch pull request meta failed");
            None
        }
    };

    // 指摘一覧への導線。アプリの公開 URL は既存の
    // `email_verification_app_url`（メール本文のリンクに使うもの）を流用する
    let base = state
        .settings
        .email_verification_app_url
        .trim_end_matches('/');
    let findings_url = (!base.is_empty()).then(|| {
        format!(
            "{base}/reviews?project={}&pr={}",
            job.project_id, job.pr_number
        )
    });

    let snapshot = service::reviews::summary_snapshot(
        &state.db,
        job.project_id,
        repo,
        job.pr_number,
        current_head_sha,
        findings_url,
    )
    .await?;
    // 現在の連携先で出したラウンドが 1 件も無ければ書かない。連携を差し替えた直後の
    // PR がこれに当たる（旧リポジトリのラウンドはこの範囲に入らない。仕様 §7）
    if snapshot.rounds == 0 {
        tracing::debug!(
            project_id = %job.project_id,
            pr = job.pr_number,
            "no round for the linked repository; skipping review summary"
        );
        return Ok(());
    }

    // マーカーはプロジェクトごと。同じリポジトリを見る別プロジェクトのコメントと
    // 取り違えない（仕様 §7）
    let marker = service::github::pr_comments::summary_marker(job.project_id);
    let updated_at = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
    let body = service::reviews::render_summary_comment(&snapshot, &marker, &updated_at);

    // 控えがあれば探索を飛ばす。無ければ「マーカー一致 かつ 自分の bot」で探す
    let known_comment_id =
        service::reviews::summary_comment_id(&state.db, job.project_id, repo, job.pr_number)
            .await?;
    let bot_login = format!("{}[bot]", github.github_app_name);

    let comment_id = service::github::pr_comments::upsert_summary_comment(
        &state.http_client,
        &token,
        &service::github::pr_comments::SummaryCommentTarget {
            owner: &integration.repo_owner,
            repo: &integration.repo_name,
            number: job.pr_number,
            marker: &marker,
            bot_login: &bot_login,
            known_comment_id,
            body: &body,
        },
    )
    .await?;

    if known_comment_id != Some(comment_id) {
        service::reviews::cache_summary_comment_id(
            &state.db,
            job.project_id,
            repo,
            job.pr_number,
            comment_id,
        )
        .await?;
    }

    tracing::info!(
        project_id = %job.project_id,
        pr = job.pr_number,
        comment_id,
        "review summary comment updated"
    );
    Ok(())
}

pub fn worker_concurrency(settings: &Settings) -> usize {
    settings.github_webhook_worker_concurrency
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ジョブペイロードは Postgres の apalis.jobs に平文で永続化されるため、
    /// トークン等の機微情報を含めてはならない（ID と PR 番号だけを載せる）。
    #[test]
    fn payload_contains_no_sensitive_fields() {
        let job = ReviewSummaryJob {
            project_id: Uuid::new_v4(),
            pr_number: 618,
        };
        let value = serde_json::to_value(&job).expect("serialize job");
        let mut keys: Vec<&str> = value
            .as_object()
            .expect("payload is a JSON object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["pr_number", "project_id"]);
    }
}
