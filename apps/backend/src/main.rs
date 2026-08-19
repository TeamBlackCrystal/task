use backend::{AppState, server::run, utils::smtp::SmtpClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings = backend::settings::load_settings()?;
    let _guard = if let Some(ref sentry_dsn) = settings.sentry_dsn {
        Some(sentry::init(sentry::ClientOptions {
            dsn: Some(sentry_dsn.parse()?),
            release: sentry::release_name!(),
            // 本番既定は false。必要時のみ環境変数で opt-in 推奨
            send_default_pii: false,
            ..Default::default()
        }))
    } else {
        None
    };
    let db = sea_orm::Database::connect(&settings.database_url).await?;
    db.get_schema_registry("entity::*").sync(&db).await?;

    let smtp_client = SmtpClient::new(
        &settings.smtp_host,
        settings.smtp_port,
        &settings.smtp_username,
        &settings.smtp_password,
        &settings.smtp_from,
    )
     .map_err(|err| {
         std::io::Error::other(format!(
             "SMTP client initialization failed. If email is required in this environment, check smtp_host/smtp_port/smtp_username/smtp_password. Underlying error: {err}"
         ))
     })?;
    let redis_client = common::cache::redis::RedisConnection::new(&settings.redis_url);
    redis_client.ping().await?;

    let pg_pool = backend::jobs::setup_pool(&settings.database_url).await?;
    let verification_email_storage =
        backend::jobs::setup_verification_email_storage(&pg_pool, &settings).await?;
    let github_webhook_storage =
        backend::jobs::setup_github_webhook_storage(&pg_pool, &settings).await?;
    let github_issue_sync_storage =
        backend::jobs::setup_github_issue_sync_storage(&pg_pool, &settings).await?;
    let password_reset_email_storage =
        backend::jobs::setup_password_reset_email_storage(&pg_pool, &settings).await?;
    let already_registered_email_storage =
        backend::jobs::setup_already_registered_email_storage(&pg_pool, &settings).await?;

    let storage = backend::utils::storage::setup_storage().await.map_err(|e| {
        std::io::Error::other(format!(
            "storage backend initialization failed (STORAGE_BACKEND / S3_* / LOCAL_UPLOAD_DIR): {e}"
        ))
    })?;

    let drive_config = backend::utils::drive::DriveConfig::from_env();
    let oauth_settings = backend::utils::oauth::OAuthSettings::from_env()?;
    let http_client = backend::utils::http::create_http_client().map_err(|err| {
        std::io::Error::other(format!("HTTP client initialization failed: {err}"))
    })?;
    let webauthn = backend::utils::webauthn::build_webauthn(&settings).map_err(|e| {
        std::io::Error::other(format!(
            "WebAuthn initialization failed (email_verification_app_url / WEBAUTHN_RP_ID): {e}"
        ))
    })?;

    // 起動時: システム上限を超過しているテナントを警告ログに出力
    if let Some(system_max) = drive_config.system_max_bytes_opt() {
        use entity::tenants;
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
        let violators = tenants::Entity::find()
            .filter(tenants::Column::DriveQuotaBytes.gt(system_max))
            .all(&db)
            .await?;
        for t in violators {
            tracing::warn!(
                tenant_id = %t.id,
                quota_bytes = ?t.drive_quota_bytes,
                system_max_bytes = system_max,
                "tenant drive quota exceeds system_max — update tenant quota or raise DRIVE_SYSTEM_MAX_QUOTA_MB"
            );
        }
    }

    if let Some(ref email) = settings.bootstrap_admin_email {
        backend::utils::bootstrap_admin::bootstrap_admin_email(&db, email).await?;
    }

    let state = AppState {
        settings,
        db,
        pg_pool,
        redis_client,
        smtp_client,
        verification_email_storage,
        github_webhook_storage,
        github_issue_sync_storage,
        password_reset_email_storage,
        already_registered_email_storage,
        storage,
        drive_config,
        oauth_settings,
        http_client,
        webauthn,
    };
    run(state).await?;

    Ok(())
}
