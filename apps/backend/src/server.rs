use apalis::layers::WorkerBuilderExt;
use apalis::layers::retry::RetryPolicy;
use apalis::prelude::WorkerBuilder;
use apalis_board::axum::{
    framework::{ApiBuilder, RegisterRoute},
    sse::{TracingBroadcaster, TracingSubscriber},
    ui::ServeUI,
};
use axum::{
    Extension, Router,
    body::Body,
    http::{HeaderValue, Method, Request},
    middleware,
    response::Redirect,
    routing::get,
};
use axum_session::{SameSite, SessionConfig, SessionLayer, SessionStore};
use axum_session_redispool::SessionRedisPool;
use sentry::integrations::tower::NewSentryLayer;
use tokio::sync::watch;
use tower::ServiceBuilder;
use tower_http::cors::{AllowHeaders, CorsLayer};
use tracing::{info, warn};
use tracing_subscriber::{Layer, layer::SubscriberExt, util::SubscriberInitExt};
use utoipa_scalar::{Scalar, Servable};

use handler::AppState;
use handler::middlewares::logging::logging_middleware;
use job::{
    JobState,
    already_registered_email::{
        self, MAX_RETRIES as ALREADY_REGISTERED_MAX_RETRIES, QUEUE_NAME as ALREADY_REGISTERED_QUEUE,
    },
    github_issue_sync::{self, QUEUE_NAME as GITHUB_ISSUE_SYNC_QUEUE},
    github_webhook::{self, QUEUE_NAME as GITHUB_WEBHOOK_QUEUE},
    password_reset_email::{
        self, MAX_RETRIES as PW_RESET_MAX_RETRIES, QUEUE_NAME as PW_RESET_QUEUE,
    },
    verification_email::{self, MAX_RETRIES, QUEUE_NAME},
};

pub async fn run(state: AppState) -> Result<(), Box<dyn std::error::Error>> {
    let log_filter = tracing_subscriber::EnvFilter::new(
        std::env::var("RUST_LOG").unwrap_or_else(|_| "info,sqlx=warn".into()),
    );

    let broadcaster = TracingBroadcaster::create();
    let board_tracing = TracingSubscriber::new(&broadcaster)
        .layer()
        .with_filter(log_filter.clone());

    tracing_subscriber::registry()
        .with(log_filter)
        .with(tracing_subscriber::fmt::layer())
        .with(board_tracing)
        .init();

    let is_prod = std::env::var("RUST_ENV").unwrap_or_default() == "production";
    let settings = &state.settings;
    let addr = settings.listen_addr.clone();

    let session_config = SessionConfig::default()
        .with_secure(is_prod)
        .with_cookie_same_site(if is_prod {
            SameSite::None
        } else {
            SameSite::Lax
        });

    let session_store = SessionStore::<SessionRedisPool>::new(
        Some(state.redis_client.conn.clone().into()),
        session_config,
    )
    .await?;

    let (router, mut openapi) = utoipa_axum::router::OpenApiRouter::new()
        .merge(handler::routes::create_routes())
        .split_for_parts();

    handler::openapi::register_schemas(&mut openapi);

    let cors = CorsLayer::new()
        .allow_origin(settings.allow_origin.parse::<HeaderValue>()?)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
        ])
        .allow_headers(AllowHeaders::mirror_request())
        .allow_credentials(true);

    let email_storage = state.verification_email_storage.as_ref().clone();
    let board_api = ApiBuilder::new(Router::new())
        .register(email_storage)
        .build();

    // ワーカーには AppState 全体ではなく、実際に使う依存だけを渡す（job → handler の循環回避）。
    let job_state = JobState {
        settings: state.settings.clone(),
        db: state.db.clone(),
        redis_client: state.redis_client.clone(),
        smtp_client: state.smtp_client.clone(),
        http_client: state.http_client.clone(),
    };

    let email_worker_storage = state.verification_email_storage.as_ref().clone();
    let worker_state = job_state.clone();
    let worker_concurrency = verification_email::worker_concurrency(settings);
    let email_worker = WorkerBuilder::new(format!("{QUEUE_NAME}-worker"))
        .backend(email_worker_storage)
        .retry(RetryPolicy::retries(MAX_RETRIES))
        .enable_tracing()
        .concurrency(worker_concurrency)
        .data(worker_state)
        .build(verification_email::process);

    let pw_reset_worker_storage = state.password_reset_email_storage.as_ref().clone();
    let pw_reset_worker_state = job_state.clone();
    let pw_reset_worker = WorkerBuilder::new(format!("{PW_RESET_QUEUE}-worker"))
        .backend(pw_reset_worker_storage)
        .retry(RetryPolicy::retries(PW_RESET_MAX_RETRIES))
        .enable_tracing()
        .concurrency(password_reset_email::worker_concurrency(settings))
        .data(pw_reset_worker_state)
        .build(password_reset_email::process);

    let already_registered_worker_storage = state.already_registered_email_storage.as_ref().clone();
    let already_registered_worker_state = job_state.clone();
    let already_registered_worker =
        WorkerBuilder::new(format!("{ALREADY_REGISTERED_QUEUE}-worker"))
            .backend(already_registered_worker_storage)
            .retry(RetryPolicy::retries(ALREADY_REGISTERED_MAX_RETRIES))
            .enable_tracing()
            .concurrency(already_registered_email::worker_concurrency(settings))
            .data(already_registered_worker_state)
            .build(already_registered_email::process);

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let worker_shutdown = shutdown_rx.clone();
    let github_worker_storage = state.github_webhook_storage.as_ref().clone();
    let github_worker_state = job_state.clone();
    let github_worker = WorkerBuilder::new(format!("{GITHUB_WEBHOOK_QUEUE}-worker"))
        .backend(github_worker_storage)
        .retry(RetryPolicy::retries(github_webhook::MAX_RETRIES))
        .enable_tracing()
        .concurrency(github_webhook::worker_concurrency(settings))
        .data(github_worker_state)
        .build(github_webhook::process);

    let issue_sync_worker_storage = state.github_issue_sync_storage.as_ref().clone();
    let issue_sync_worker = WorkerBuilder::new(format!("{GITHUB_ISSUE_SYNC_QUEUE}-worker"))
        .backend(issue_sync_worker_storage)
        .retry(RetryPolicy::retries(github_issue_sync::MAX_RETRIES))
        .enable_tracing()
        .concurrency(github_issue_sync::worker_concurrency(settings))
        .data(job_state)
        .build(github_issue_sync::process);

    let github_shutdown = shutdown_rx.clone();
    let github_worker_handle = tokio::spawn(async move {
        github_worker
            .run_until(wait_for_shutdown(github_shutdown))
            .await
    });

    let issue_sync_shutdown = shutdown_rx.clone();
    let issue_sync_worker_handle = tokio::spawn(async move {
        issue_sync_worker
            .run_until(wait_for_shutdown(issue_sync_shutdown))
            .await
    });

    // 書き戻し要求（pending_push）の取りこぼしを定期的に拾い直す。
    // タスク更新後のジョブ登録が失敗しても、要求は DB に残っているのでここで回収する。
    let sweep_db = state.db.clone();
    let sweep_storage = state.github_issue_sync_storage.clone();
    let mut sweep_shutdown = shutdown_rx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(
            github_issue_sync::SWEEP_INTERVAL_SECS,
        ));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    match github_issue_sync::sweep_pending(&sweep_db, &sweep_storage).await {
                        Ok(0) => {}
                        Ok(n) => info!(count = n, "github issue push sweep re-enqueued"),
                        Err(e) => warn!(error = %e, "github issue push sweep failed"),
                    }
                }
                _ = sweep_shutdown.changed() => break,
            }
        }
    });

    let pw_reset_shutdown = shutdown_rx.clone();
    let email_worker_handle = tokio::spawn(async move {
        email_worker
            .run_until(wait_for_shutdown(worker_shutdown))
            .await
    });
    let pw_reset_worker_handle = tokio::spawn(async move {
        pw_reset_worker
            .run_until(wait_for_shutdown(pw_reset_shutdown))
            .await
    });

    let already_registered_shutdown = shutdown_rx.clone();
    let already_registered_worker_handle = tokio::spawn(async move {
        already_registered_worker
            .run_until(wait_for_shutdown(already_registered_shutdown))
            .await
    });

    let api = router
        .merge(Scalar::with_url("/scalar", openapi.clone()))
        .with_state(state.clone())
        .layer(cors)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            handler::middlewares::csrf::csrf_origin_check,
        ))
        .layer(middleware::from_fn(logging_middleware))
        .layer(SessionLayer::new(session_store.clone()))
        .layer(ServiceBuilder::new().layer(NewSentryLayer::<Request<Body>>::new_from_top()));

    // apalis-board の UI はビルド時に API=/api/v1・静的ファイル=/ 直下を前提とする。
    // /jobs にネストすると JS/WASM が 404 になり真っ白になる。
    // ジョブペイロード（メール認証トークン・メールアドレス）と SSE ログを露出するため、
    // board 系ルートはすべて管理者セッション必須。board API は再試行等の状態変更を持ち
    // 公開 API と同じ SameSite=None セッション Cookie で認証するため、CSRF 検査も api と同様に通す。
    let board = Router::new()
        .nest("/api/v1", board_api)
        .route("/jobs", get(|| async { Redirect::permanent("/") }))
        .route("/jobs/", get(|| async { Redirect::permanent("/") }))
        .fallback_service(ServeUI::new())
        .layer(Extension(broadcaster))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            handler::middlewares::admin_gate::require_admin_session,
        ))
        .layer(middleware::from_fn_with_state(
            state,
            handler::middlewares::csrf::csrf_origin_check,
        ))
        .layer(SessionLayer::new(session_store));

    let app = Router::new().merge(api).merge(board);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("Listening on http://{addr}");
    info!("Apalis board: http://{addr}/");

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal_inner().await;
            let _ = shutdown_tx.send(true);
            info!("shutting down HTTP server; Apalis workers finishing in-flight jobs");
        })
        .await?;

    match email_worker_handle.await {
        Ok(Ok(())) => info!("verification email worker stopped"),
        Ok(Err(e)) => warn!("verification email worker error: {e}"),
        Err(e) => warn!("verification email worker join error: {e}"),
    }
    match pw_reset_worker_handle.await {
        Ok(Ok(())) => info!("password reset email worker stopped"),
        Ok(Err(e)) => warn!("password reset email worker error: {e}"),
        Err(e) => warn!("password reset email worker join error: {e}"),
    }
    match already_registered_worker_handle.await {
        Ok(Ok(())) => info!("already registered email worker stopped"),
        Ok(Err(e)) => warn!("already registered email worker error: {e}"),
        Err(e) => warn!("already registered email worker join error: {e}"),
    }

    match github_worker_handle.await {
        Ok(Ok(())) => info!("github webhook worker stopped"),
        Ok(Err(e)) => warn!("github webhook worker error: {e}"),
        Err(e) => warn!("github webhook worker join error: {e}"),
    }

    match issue_sync_worker_handle.await {
        Ok(Ok(())) => info!("github issue sync worker stopped"),
        Ok(Err(e)) => warn!("github issue sync worker error: {e}"),
        Err(e) => warn!("github issue sync worker join error: {e}"),
    }

    Ok(())
}

async fn wait_for_shutdown(mut shutdown: watch::Receiver<bool>) -> Result<(), std::io::Error> {
    while !*shutdown.borrow() {
        if shutdown.changed().await.is_err() {
            break;
        }
    }
    Ok(())
}

async fn shutdown_signal_inner() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{SignalKind, signal};

        let mut sigterm =
            signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");
        sigterm.recv().await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => warn!("received Ctrl+C"),
        () = terminate => warn!("received SIGTERM"),
    }
}
