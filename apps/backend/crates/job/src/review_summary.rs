//! レビュー指摘の要約を PR コメントへ反映するジョブ。
//!
//! ラウンドの起票時と指摘の状態遷移時に投入される。GitHub には
//! マーカー付きのコメント 1 本だけを置き、以後は同じコメントを編集する
//! （仕様 `docs/features/review-findings.md` §7）。
//!
//! 失敗はベストエフォート: 投稿・編集に失敗しても API 側の起票・遷移は
//! 巻き戻さない。GitHub 連携の無いプロジェクトでは何もしない。

use std::sync::Arc;
use std::time::Duration;

use apalis::prelude::{
    BackoffConfig, BoxDynError, Data, IntervalStrategy, StrategyBuilder, TaskSink,
};
use apalis_postgres::{Config, JsonCodec, PgPool, PostgresStorage};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use common::settings::Settings;
use entity::github_integrations;

use crate::JobState;

pub const QUEUE_NAME: &str = "review_summary";
pub const MAX_RETRIES: usize = 3;

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
                IntervalStrategy::new(Duration::from_secs(2))
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

/// 投稿に失敗してもジョブ側の呼び出し元（API）を巻き込まないよう、
/// enqueue の失敗は警告に留める。
pub async fn enqueue_best_effort(storage: &ReviewSummaryStorage, project_id: Uuid, pr_number: i32) {
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
    }
}

pub async fn process(job: ReviewSummaryJob, state: Data<JobState>) -> Result<(), BoxDynError> {
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
    // （PR 番号だけで用は足りるので、ここで止めると本題を落とす）
    match service::github::pr_comments::fetch_pull_request(
        &state.http_client,
        &token,
        &integration.repo_owner,
        &integration.repo_name,
        job.pr_number,
    )
    .await
    {
        Ok(meta) => {
            service::reviews::cache_pr_meta(
                &state.db,
                job.project_id,
                job.pr_number,
                &meta.title,
                meta.user.as_ref().map(|u| u.login.as_str()),
            )
            .await?;
        }
        Err(e) => {
            tracing::warn!(error = %e, pr = job.pr_number, "fetch pull request meta failed");
        }
    }

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

    let snapshot =
        service::reviews::summary_snapshot(&state.db, job.project_id, job.pr_number, findings_url)
            .await?;
    let updated_at = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
    let body = service::reviews::render_summary_comment(&snapshot, &updated_at);

    let comment_id = service::github::pr_comments::upsert_summary_comment(
        &state.http_client,
        &token,
        &integration.repo_owner,
        &integration.repo_name,
        job.pr_number,
        &body,
    )
    .await?;

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
