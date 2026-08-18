//! GitHub Webhook イベント受信ジョブ（Wave 0: 受信確認のみ）。

use std::sync::Arc;
use std::time::Duration;

use apalis::prelude::{
    BackoffConfig, BoxDynError, Data, IntervalStrategy, StrategyBuilder, TaskSink,
};
use apalis_postgres::{Config, JsonCodec, PgPool, PostgresStorage};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use common::settings::Settings;

use crate::JobState;

pub const QUEUE_NAME: &str = "github_webhook";
pub const MAX_RETRIES: usize = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubWebhookJob {
    pub integration_id: Uuid,
    pub project_id: Uuid,
    pub event: String,
    pub delivery_id: Option<String>,
    pub payload: serde_json::Value,
}

pub type GithubWebhookStorage = PostgresStorage<
    GithubWebhookJob,
    apalis_postgres::CompactType,
    JsonCodec<apalis_postgres::CompactType>,
    apalis_postgres::PgNotify,
>;

pub fn build_storage(pool: &PgPool, _settings: &Settings) -> GithubWebhookStorage {
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
) -> Result<Arc<GithubWebhookStorage>, anyhow::Error> {
    PostgresStorage::setup(pool).await?;
    Ok(Arc::new(build_storage(pool, settings)))
}

pub async fn enqueue(
    storage: &GithubWebhookStorage,
    job: GithubWebhookJob,
) -> Result<(), anyhow::Error> {
    let mut storage = storage.clone();
    storage
        .push(job)
        .await
        .map_err(|e| anyhow::anyhow!("push github webhook job: {e}"))?;
    Ok(())
}

/// `issues` イベントはタスクへ反映する。それ以外は受信をログに残すのみ。
pub async fn process(job: GithubWebhookJob, state: Data<JobState>) -> Result<(), BoxDynError> {
    if job.event == "issues" {
        let applied =
            service::github::sync::apply_issue_event(&state.db, job.project_id, &job.payload)
                .await?;
        tracing::info!(
            integration_id = %job.integration_id,
            project_id = %job.project_id,
            delivery_id = ?job.delivery_id,
            applied,
            "github issues event processed"
        );
        return Ok(());
    }

    tracing::info!(
        integration_id = %job.integration_id,
        project_id = %job.project_id,
        event = %job.event,
        delivery_id = ?job.delivery_id,
        "github webhook received (wave 0 ack)"
    );
    Ok(())
}

pub fn worker_concurrency(settings: &Settings) -> usize {
    settings.github_webhook_worker_concurrency
}
