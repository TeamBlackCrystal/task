//! HTTP ハンドラー層: axum ハンドラー・ルーティング・抽出器・ミドルウェア
//! （旧 backend 直下、#151 Phase 4）。

use std::sync::Arc;

use apalis_postgres::PgPool;
use common::cache::redis::RedisConnection;
use common::settings::Settings;
use job::{
    AlreadyRegisteredEmailStorage, GithubWebhookStorage, PasswordResetEmailStorage,
    VerificationEmailStorage,
};
use sea_orm::DatabaseConnection;
use service::{
    drive::DriveConfig, oauth::OAuthSettings, smtp::SmtpClient, storage::StorageBackend,
};
use webauthn_rs::prelude::Webauthn;

// 旧 crate::error / crate::settings パス互換のための再公開。
pub use common::{error, settings};

pub mod auth_helpers;
pub mod extractors;
pub mod handlers;
pub mod middlewares;
pub mod openapi;
pub mod routes;

#[derive(Clone)]
pub struct AppState {
    pub settings: Settings,
    pub db: DatabaseConnection,
    pub pg_pool: PgPool,
    pub redis_client: RedisConnection,
    pub smtp_client: SmtpClient,
    pub verification_email_storage: Arc<VerificationEmailStorage>,
    pub github_webhook_storage: Arc<GithubWebhookStorage>,
    pub password_reset_email_storage: Arc<PasswordResetEmailStorage>,
    pub already_registered_email_storage: Arc<AlreadyRegisteredEmailStorage>,
    pub storage: Arc<dyn StorageBackend>,
    pub drive_config: DriveConfig,
    pub oauth_settings: OAuthSettings,
    pub http_client: reqwest::Client,
    pub webauthn: Arc<Webauthn>,
}
