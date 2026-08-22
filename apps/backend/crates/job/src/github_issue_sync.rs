//! GitHub Issue ↔ タスクの同期ジョブ。
//!
//! 初回の一括インポートと、タスク側の変更の書き戻しを同じキューで扱う。
//! GitHub 側の変更の取り込みは webhook 経由なので [`crate::github_webhook`] にある。

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

pub const QUEUE_NAME: &str = "github_issue_sync";
pub const MAX_RETRIES: usize = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GithubIssueSyncJob {
    /// リポジトリの Issue を全件取り込む。
    Import { project_id: Uuid },
    /// タスク側の変更を、リンク済み Issue へ書き戻す。
    Push { task_id: Uuid },
}

pub type GithubIssueSyncStorage = PostgresStorage<
    GithubIssueSyncJob,
    apalis_postgres::CompactType,
    JsonCodec<apalis_postgres::CompactType>,
    apalis_postgres::PgNotify,
>;

pub fn build_storage(pool: &PgPool, _settings: &Settings) -> GithubIssueSyncStorage {
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
) -> Result<Arc<GithubIssueSyncStorage>, anyhow::Error> {
    PostgresStorage::setup(pool).await?;
    Ok(Arc::new(build_storage(pool, settings)))
}

pub async fn enqueue(
    storage: &GithubIssueSyncStorage,
    job: GithubIssueSyncJob,
) -> Result<(), anyhow::Error> {
    let mut storage = storage.clone();
    storage
        .push(job)
        .await
        .map_err(|e| anyhow::anyhow!("push github issue sync job: {e}"))?;
    Ok(())
}

pub async fn process(job: GithubIssueSyncJob, state: Data<JobState>) -> Result<(), BoxDynError> {
    let Some(github) = state.settings.github_app.as_ref() else {
        tracing::warn!("github app is not configured; skipping issue sync");
        return Ok(());
    };

    match job {
        GithubIssueSyncJob::Import { project_id } => {
            let result =
                service::github::import_project(&state.db, &state.http_client, github, project_id)
                    .await;
            // 成否によらず取り込み枠を返す（失敗のまま塞ぐと、ユーザー自身の
            // やり直しまで TTL のあいだ弾かれる）
            if let Err(e) =
                service::github::release_import_slot(&state.redis_client, project_id).await
            {
                tracing::warn!(error = %e, %project_id, "release github import lock failed");
            }
            let imported = result?;
            tracing::info!(%project_id, imported, "github issue import finished");
        }
        GithubIssueSyncJob::Push { task_id } => {
            service::github::push_task(&state.db, &state.http_client, github, task_id).await?;
        }
    }
    Ok(())
}

pub fn worker_concurrency(settings: &Settings) -> usize {
    settings.github_webhook_worker_concurrency
}

/// pending_push の残留分を拾って書き戻しジョブを積み直すスイープ間隔。
pub const SWEEP_INTERVAL_SECS: u64 = 300;

/// タスク更新時のジョブ登録に失敗した書き戻し要求（pending_push が立ったまま
/// のリンク）を拾い直す。要求はタスク更新と同一トランザクションで永続化されて
/// いるため、ここで再登録すれば取りこぼしがない。二重登録になっても
/// `push_task` がハッシュ比較で no-op になるだけで害はない。
pub async fn sweep_pending(
    db: &sea_orm::DatabaseConnection,
    storage: &GithubIssueSyncStorage,
) -> Result<usize, anyhow::Error> {
    let task_ids = service::github::sync::find_pending_push_tasks(db).await?;
    let count = task_ids.len();
    for task_id in task_ids {
        enqueue(storage, GithubIssueSyncJob::Push { task_id }).await?;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ジョブペイロードは Postgres の apalis.jobs に平文で永続化されるため、
    /// アクセストークン等の機微情報を含めてはならない（ID だけを載せる）。
    /// フィールド追加でこのテストが落ちた場合は、機微情報でないことを
    /// 確認したうえで期待キー集合を更新すること。
    #[test]
    fn payload_contains_no_sensitive_fields() {
        for (job, expected) in [
            (
                GithubIssueSyncJob::Import {
                    project_id: Uuid::new_v4(),
                },
                vec!["kind", "project_id"],
            ),
            (
                GithubIssueSyncJob::Push {
                    task_id: Uuid::new_v4(),
                },
                vec!["kind", "task_id"],
            ),
        ] {
            let value = serde_json::to_value(&job).expect("serialize job");
            let mut keys: Vec<&str> = value
                .as_object()
                .expect("payload is a JSON object")
                .keys()
                .map(String::as_str)
                .collect();
            keys.sort_unstable();
            assert_eq!(keys, expected);
        }
    }
}
