//! Apalis バックグラウンドジョブ

pub mod already_registered_email;
pub mod github_issue_sync;
pub mod github_webhook;
pub mod password_reset_email;
pub mod review_summary;
pub mod verification_email;

use std::sync::Arc;

use apalis_postgres::PgPool;

use common::settings::Settings;

pub use already_registered_email::{AlreadyRegisteredEmailJob, AlreadyRegisteredEmailStorage};
pub use github_issue_sync::{GithubIssueSyncJob, GithubIssueSyncStorage};
pub use github_webhook::{GithubWebhookJob, GithubWebhookStorage};
pub use password_reset_email::{PasswordResetEmailJob, PasswordResetEmailStorage};
pub use review_summary::{ReviewSummaryJob, ReviewSummaryStorage};
pub use verification_email::{
    MAX_RETRIES, QUEUE_NAME, VerificationEmailJob, VerificationEmailStorage,
};

/// ワーカーが必要とする依存の束。
/// AppState（handler クレート）を受け取ると job → handler の循環になるため、
/// ワーカーは実際に使う要素だけをここから受け取る。
#[derive(Clone)]
pub struct JobState {
    pub settings: Settings,
    pub db: sea_orm::DatabaseConnection,
    pub redis_client: common::cache::redis::RedisConnection,
    pub smtp_client: service::smtp::SmtpClient,
    pub http_client: reqwest::Client,
}

pub async fn setup_pool(database_url: &str) -> Result<PgPool, anyhow::Error> {
    Ok(PgPool::connect(database_url).await?)
}

pub async fn setup_verification_email_storage(
    pool: &PgPool,
    settings: &Settings,
) -> Result<Arc<VerificationEmailStorage>, anyhow::Error> {
    verification_email::setup(pool, settings).await
}

pub async fn setup_github_webhook_storage(
    pool: &PgPool,
    settings: &Settings,
) -> Result<Arc<GithubWebhookStorage>, anyhow::Error> {
    github_webhook::setup(pool, settings).await
}

pub async fn setup_github_issue_sync_storage(
    pool: &PgPool,
    settings: &Settings,
) -> Result<Arc<GithubIssueSyncStorage>, anyhow::Error> {
    github_issue_sync::setup(pool, settings).await
}

pub async fn setup_review_summary_storage(
    pool: &PgPool,
    settings: &Settings,
) -> Result<Arc<ReviewSummaryStorage>, anyhow::Error> {
    review_summary::setup(pool, settings).await
}

pub async fn setup_password_reset_email_storage(
    pool: &PgPool,
    settings: &Settings,
) -> Result<Arc<PasswordResetEmailStorage>, anyhow::Error> {
    password_reset_email::setup(pool, settings).await
}

pub async fn setup_already_registered_email_storage(
    pool: &PgPool,
    settings: &Settings,
) -> Result<Arc<AlreadyRegisteredEmailStorage>, anyhow::Error> {
    already_registered_email::setup(pool, settings).await
}
