//! ビジネスロジック/横断サービス層（旧 src/utils、#151 Phase 4）。

// 旧 crate::error / crate::settings パス互換のための再公開。
pub use common::{error, settings};

pub mod access;
pub mod already_registered_email_delivery;
pub mod auth;
pub mod bootstrap_admin;
pub mod custom_fields;
pub mod drive;
pub mod email;
pub mod email_verification;
pub mod github;
pub mod http;
pub mod login_session;
pub mod notifications;
pub mod oauth;
pub mod passkey_challenges;
pub mod passkeys;
pub mod password_reset;
pub mod password_reset_delivery;
pub mod password_reset_email_delivery;
pub mod password_reset_log;
pub mod smtp;
pub mod storage;
pub mod task_activities;
pub mod task_responses;
pub mod totp;
pub mod verification_email_delivery;
pub mod webauthn;

pub use common::db;
pub use common::system_settings;
pub use common::validation;
