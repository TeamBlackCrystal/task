//! HTTP 統合テスト用の Axum アプリ構築ヘルパー（admin / GitHub App 共通）。

// 各統合テストバイナリごとにコンパイルされるため、一部バイナリで未使用のヘルパーが
// dead_code 誤検知になるのを抑止する。
#![allow(dead_code)]

use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use axum::{
    Json, Router,
    body::Body,
    extract::{Query, State},
    http::{Request, StatusCode, header},
    response::{IntoResponse, Redirect},
    routing::{get, post},
};
use axum_session::{SameSite, SessionConfig, SessionLayer, SessionMode, SessionStore};
use axum_session_redispool::SessionRedisPool;
use entity::{github_integrations, oauth_connections, projects, tenant_members, tenants, users};

use backend::{
    AppState,
    jobs::{
        setup_already_registered_email_storage, setup_github_issue_sync_storage,
        setup_github_webhook_storage, setup_password_reset_email_storage, setup_pool,
        setup_review_summary_storage, setup_verification_email_storage,
    },
    routes, settings,
    utils::{
        auth::create_password_hash,
        drive::DriveConfig,
        http::create_http_client,
        oauth::{OAuthSettings, ProviderConfig},
        smtp::SmtpClient,
        storage::setup_storage,
        totp::build_totp,
        webauthn::build_webauthn,
    },
};
use common::cache::redis::RedisConnection;
use cookie::Key;
use http_body_util::BodyExt;
use reqwest::{Client, Response, redirect::Policy};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseConnection,
    EntityTrait, QueryFilter,
};
use serde::{Deserialize, Serialize};
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{
        Container, GenericImage, ImageExt,
        core::{IntoContainerPort, WaitFor},
        runners::SyncRunner,
    },
};
use tokio::{net::TcpListener, sync::OnceCell};
use tower::ServiceExt;
use url::Url;
use uuid::Uuid;

static SCHEMA_READY: OnceCell<()> = OnceCell::const_new();

fn init_tracing() {
    static TRACING: OnceLock<()> = OnceLock::new();
    TRACING.get_or_init(|| {
        let _ = tracing_subscriber::fmt()
            .with_env_filter("backend=debug")
            .with_test_writer()
            .try_init();
    });
}

pub fn is_redirect(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::MOVED_PERMANENTLY
            | StatusCode::FOUND
            | StatusCode::SEE_OTHER
            | StatusCode::TEMPORARY_REDIRECT
            | StatusCode::PERMANENT_REDIRECT
    )
}

pub const TEST_OAUTH_CLIENT_ID: &str = "test-gitlab-selfhosted-client";
pub const TEST_OAUTH_CLIENT_SECRET: &str = "test-gitlab-selfhosted-secret";
pub const TEST_OAUTH_ENCRYPTION_KEY: &str = "01234567890123456789012345678901";

fn load_dotenv() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let _ = dotenvy::dotenv().ok();
    let _ = dotenvy::from_path(manifest.join(".env")).ok();
    let _ = dotenvy::from_path("/home/coder/task/apps/backend/.env").ok();
}

static TEST_ENV_READY: OnceLock<()> = OnceLock::new();

/// 統合テストに必要な環境変数を整える（テストバイナリごとに1回だけ実行）。
///
/// - `.env` / プロセス環境に `DATABASE_URL` / `REDIS_URL` があればそのまま使う（CI 経路）
/// - 無ければ testcontainers で CI と同一イメージ（`postgres:17` / `valkey/valkey:8.1`）を
///   ランダムポートで起動し、接続文字列を注入する
/// - SMTP 等の残りの必須 env は `backend-test.yml` の `env:` と同じテスト用の値で補完する
fn ensure_test_env() {
    TEST_ENV_READY.get_or_init(|| {
        load_dotenv();

        let need_pg = std::env::var("DATABASE_URL").is_err();
        let need_redis = std::env::var("REDIS_URL").is_err();
        if need_pg || need_redis {
            // SyncRunner は内部で tokio ランタイムを使うため、テストのランタイム上から
            // 直接呼ぶとパニックする。専用スレッドで起動して結果だけ受け取る。
            let (pg_url, redis_url) =
                std::thread::spawn(move || start_test_containers(need_pg, need_redis))
                    .join()
                    .expect("start test containers");
            // SAFETY: test process is single-threaded before Tokio workers spawn.
            unsafe {
                if let Some(url) = pg_url {
                    std::env::set_var("DATABASE_URL", url);
                }
                if let Some(url) = redis_url {
                    std::env::set_var("REDIS_URL", url);
                }
            }
        }

        // `.env` なしでも動くよう、CI (`backend-test.yml`) と同じテスト用デフォルトを補完する。
        set_default_env("SMTP_HOST", "localhost");
        set_default_env("SMTP_PORT", "587");
        set_default_env("SMTP_USERNAME", "test");
        set_default_env("SMTP_PASSWORD", "test");
        set_default_env("SMTP_FROM", "test@example.com");
        set_default_env("EMAIL_VERIFICATION_APP_URL", "http://localhost:3000");
        set_default_env("PERSONAL_TOKEN_SECRET", "00000000000000000000000000000000");
        set_default_env("RECOVERY_CODE_SECRET", "00000000000000000000000000000000");
        set_default_env("TOTP_ENCRYPTION_KEY", "01234567890123456789012345678901");
        set_default_env("ARGON2_TEST_MODE", "true");
    });
}

fn set_default_env(key: &str, value: &str) {
    if std::env::var(key).is_err() {
        // SAFETY: test process is single-threaded before Tokio workers spawn.
        unsafe { std::env::set_var(key, value) };
    }
}

/// テストバイナリのプロセス生存期間中コンテナを保持する static。
/// testcontainers-rs 0.27 に ryuk（外部リーパー）は無く、削除は `Container` の
/// Drop が担うため、`drop_test_containers`（atexit）で明示的に drop する。
static PG_CONTAINER: Mutex<Option<Container<Postgres>>> = Mutex::new(None);
static VALKEY_CONTAINER: Mutex<Option<Container<GenericImage>>> = Mutex::new(None);

/// テストプロセス終了時（テストハーネスの `process::exit` 時）にコンテナを削除する。
extern "C" fn drop_test_containers() {
    if let Ok(mut guard) = PG_CONTAINER.lock() {
        guard.take();
    }
    if let Ok(mut guard) = VALKEY_CONTAINER.lock() {
        guard.take();
    }
}

/// CI (`backend-test.yml`) と同一イメージのコンテナをランダムポートで起動する。
///
/// 戻り値は (`DATABASE_URL`, `REDIS_URL`)。コンテナはプロセス終了まで保持し、
/// atexit ハンドラが自動削除する（SIGKILL 等の異常終了時のみ手動削除が必要）。
fn start_test_containers(need_pg: bool, need_redis: bool) -> (Option<String>, Option<String>) {
    // SAFETY: 登録するのはキャプチャ無しの extern "C" 関数で、libc の規約どおり。
    unsafe {
        libc::atexit(drop_test_containers);
    }
    let pg_url = need_pg.then(|| {
        let container = Postgres::default()
            .with_tag("17")
            .start()
            .expect("start postgres testcontainer");
        let port = container
            .get_host_port_ipv4(5432)
            .expect("postgres host port");
        *PG_CONTAINER.lock().expect("pg container slot") = Some(container);
        format!("postgresql://postgres:postgres@127.0.0.1:{port}/postgres")
    });
    let redis_url = need_redis.then(|| {
        let container = GenericImage::new("valkey/valkey", "8.1")
            .with_exposed_port(6379.tcp())
            .with_wait_for(WaitFor::message_on_stdout("Ready to accept connections"))
            .start()
            .expect("start valkey testcontainer");
        let port = container
            .get_host_port_ipv4(6379)
            .expect("valkey host port");
        *VALKEY_CONTAINER.lock().expect("valkey container slot") = Some(container);
        format!("redis://127.0.0.1:{port}")
    });
    (pg_url, redis_url)
}

/// GitHub App HTTP 統合テスト用: 必須の GitHub 環境変数を設定する。
pub fn load_github_test_env() {
    ensure_test_env();
    // SAFETY: test process is single-threaded before Tokio workers spawn.
    unsafe {
        std::env::set_var("GITHUB_APP_ID", "1");
        std::env::set_var("GITHUB_APP_WEBHOOK_SECRET", "webhook-secret");
        std::env::set_var("GITHUB_APP_NAME", "task-app");
        std::env::set_var("GITHUB_APP_CLIENT_ID", "Iv1.testclientid");
        std::env::set_var("GITHUB_APP_CLIENT_SECRET", "test-client-secret");
        std::env::set_var(
            "GITHUB_TOKEN_ENCRYPTION_KEY",
            "01234567890123456789012345678901",
        );
        let pem = include_str!("../fixtures/github_test_rsa.pem").replace('\n', "\\n");
        std::env::set_var("GITHUB_APP_PRIVATE_KEY", pem);
    }
}

fn test_session_config() -> SessionConfig {
    SessionConfig::default()
        .with_secure(false)
        .with_cookie_same_site(SameSite::Lax)
        .with_ip_and_user_agent(false)
        .with_prefix_with_host(false)
        .with_mode(SessionMode::Persistent)
        .with_key(Key::from(&[7u8; 64]))
        .with_database_key(Key::from(&[8u8; 64]))
}

async fn ensure_schema(db: &DatabaseConnection) {
    SCHEMA_READY
        .get_or_init(|| async {
            // Sea-ORM の sync() は CREATE UNIQUE INDEX 由来のインデックスを
            // ALTER TABLE DROP CONSTRAINT で削除しようとして失敗するため、
            // 事前に DROP して sync() が素通りできるようにする
            let _ = db
                .execute_unprepared("DROP INDEX IF EXISTS idx_sprints_active_per_project")
                .await;
            let _ = db
                .execute_unprepared("DROP INDEX IF EXISTS idx_projects_personal_owner")
                .await;

            db.get_schema_registry("entity::*")
                .sync(db)
                .await
                .expect("sync schema");

            db.execute_unprepared(
                "ALTER TABLE users ADD COLUMN IF NOT EXISTS is_admin BOOLEAN NOT NULL DEFAULT false;
                 ALTER TABLE users ADD COLUMN IF NOT EXISTS is_suspended BOOLEAN NOT NULL DEFAULT false;
                 ALTER TABLE users ADD COLUMN IF NOT EXISTS sessions_revoked_at TIMESTAMPTZ;
                 ALTER TABLE users ALTER COLUMN password_hash DROP NOT NULL;
                 ALTER TABLE personal_tokens DROP COLUMN IF EXISTS token;",
            )
            .await
            .expect("prepare user columns for integration tests");

            db.execute_unprepared(
                r#"
ALTER TABLE users ADD COLUMN IF NOT EXISTS totp_enabled BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE tenants ADD COLUMN IF NOT EXISTS require_2fa BOOLEAN NOT NULL DEFAULT false;
UPDATE users SET totp_enabled = false WHERE totp_enabled IS NULL;
ALTER TABLE users ALTER COLUMN totp_enabled SET NOT NULL;
ALTER TABLE users ALTER COLUMN totp_enabled SET DEFAULT false;

CREATE TABLE IF NOT EXISTS totp_credentials (
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    secret_enc TEXT NOT NULL,
    is_verified BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS recovery_codes (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    code_hash VARCHAR NOT NULL,
    used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_recovery_codes_user ON recovery_codes(user_id);
"#,
            )
            .await
            .expect("prepare 2fa schema");

            db.execute_unprepared(
                r#"
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS completed_at TIMESTAMPTZ;
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS sprint_id UUID REFERENCES sprints(id) ON DELETE SET NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_sprints_active_per_project
    ON sprints(project_id)
    WHERE status = 'active';
"#,
            )
            .await
            .expect("prepare sprints schema");

            db.execute_unprepared(
                r#"
DROP INDEX IF EXISTS idx_projects_personal_owner;
ALTER TABLE projects
    ADD COLUMN IF NOT EXISTS is_personal BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS personal_owner_id UUID REFERENCES users(id) ON DELETE CASCADE;
CREATE UNIQUE INDEX IF NOT EXISTS idx_projects_personal_owner
    ON projects(tenant_id, personal_owner_id)
    WHERE is_personal = true;
"#,
            )
            .await
            .expect("prepare personal project columns");

            // sync() は entity 定義からカラムデフォルトを作らないため、
            // `ensure_system_settings_row` の `INSERT INTO system_settings (singleton)` が
            // NOT NULL 制約違反 (23502) になる。マイグレーション
            // （m20260520000000_initial_schema.rs）と同じデフォルトを付与する。
            db.execute_unprepared(
                r#"
ALTER TABLE system_settings
    ALTER COLUMN user_registration_enabled SET DEFAULT true,
    ALTER COLUMN drive_default_quota_mb SET DEFAULT 10240,
    ALTER COLUMN updated_at SET DEFAULT now(),
    ALTER COLUMN drive_system_max_quota_mb SET DEFAULT 102400;
"#,
            )
            .await
            .expect("prepare system_settings defaults");

            // search_vector は GENERATED ALWAYS AS の tsvector カラムで entity 定義に無いため
            // sync() が作らない。手動で追加する。
            db.execute_unprepared(
                r#"
ALTER TABLE tasks
    ADD COLUMN IF NOT EXISTS search_vector tsvector
    GENERATED ALWAYS AS (
        to_tsvector('pg_catalog.simple',
            coalesce(title, '') || ' ' || coalesce(description, ''))
    ) STORED;
CREATE INDEX IF NOT EXISTS idx_tasks_search_vector ON tasks USING GIN(search_vector);
"#,
            )
            .await
            .expect("prepare search_vector column");

        })
        .await;
}

#[derive(Clone, Debug)]
pub struct MockGitLabUser {
    pub id: i64,
    pub username: String,
    pub email: Option<String>,
}

#[derive(Clone)]
struct MockOAuthState {
    user: Arc<Mutex<MockGitLabUser>>,
}

#[derive(Debug, Deserialize)]
struct AuthorizeQuery {
    redirect_uri: String,
    state: String,
}

#[derive(Debug, Serialize)]
struct TokenResponseBody {
    access_token: String,
    token_type: &'static str,
}

#[derive(Debug, Serialize)]
struct GitLabUserResponse {
    id: i64,
    username: String,
    email: Option<String>,
    confirmed_at: Option<String>,
    avatar_url: Option<String>,
}

pub struct MockOAuthHandle {
    pub base_url: String,
    user: Arc<Mutex<MockGitLabUser>>,
}

impl MockOAuthHandle {
    pub async fn start(initial_user: MockGitLabUser) -> Self {
        let user = Arc::new(Mutex::new(initial_user));
        let state = MockOAuthState { user: user.clone() };

        let router = Router::new()
            .route("/oauth/authorize", get(authorize))
            .route("/oauth/token", post(token))
            .route("/api/v4/user", get(userinfo))
            .with_state(state);

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock oauth listener");
        let addr = listener.local_addr().expect("mock oauth addr");
        tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("serve mock oauth");
        });

        Self {
            base_url: format!("http://{addr}"),
            user,
        }
    }

    pub fn set_user(&self, user: MockGitLabUser) {
        *self.user.lock().expect("mock oauth user lock") = user;
    }
}

async fn authorize(Query(query): Query<AuthorizeQuery>) -> Redirect {
    let mut url =
        url::Url::parse(&query.redirect_uri).expect("mock oauth redirect_uri must be valid");
    url.query_pairs_mut()
        .append_pair("code", "mock-auth-code")
        .append_pair("state", &query.state);
    Redirect::temporary(url.as_str())
}

async fn token() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(TokenResponseBody {
            access_token: "mock-access-token".to_string(),
            token_type: "Bearer",
        }),
    )
}

async fn userinfo(State(state): State<MockOAuthState>) -> impl IntoResponse {
    let user = state.user.lock().expect("mock oauth user lock").clone();
    let confirmed_at = user
        .email
        .as_ref()
        .map(|_| "2024-01-01T00:00:00Z".to_string());
    Json(GitLabUserResponse {
        id: user.id,
        username: user.username,
        email: user.email,
        confirmed_at,
        avatar_url: None,
    })
}

pub struct TestApp {
    pub state: AppState,
    pub base_url: String,
    pub mock: MockOAuthHandle,
    client: Client,
    router: Router,
}

#[derive(Clone)]
pub struct TestUser {
    pub id: Uuid,
    pub email: String,
    pub password: String,
}

pub struct TestTenantProject {
    pub tenant_id: Uuid,
    pub project_id: Uuid,
}

pub struct Enabled2fa {
    pub recovery_codes: Vec<String>,
}

impl TestApp {
    pub async fn new() -> Self {
        init_tracing();
        ensure_test_env();
        let mut settings = settings::load_settings().expect("load settings from .env");
        settings.email_verification_app_url = "http://localhost:3000".to_string();
        Self::build(settings).await
    }

    pub async fn new_with_github() -> Self {
        init_tracing();
        load_github_test_env();
        let settings = settings::load_settings().expect("load settings");
        assert!(
            settings.github_app.is_some(),
            "GITHUB_APP_ID must enable github_app in tests"
        );
        Self::build(settings).await
    }

    async fn build(settings: settings::Settings) -> Self {
        // Each #[tokio::test] owns a separate runtime. Keep runtime-bound
        // connection pools scoped to the TestApp created on that runtime.
        let db = sea_orm::Database::connect(&settings.database_url)
            .await
            .expect("connect database");
        ensure_schema(&db).await;

        let smtp_client = SmtpClient::new(
            &settings.smtp_host,
            settings.smtp_port,
            &settings.smtp_username,
            &settings.smtp_password,
            &settings.smtp_from,
        )
        .expect("smtp client");
        let redis_client = RedisConnection::new(&settings.redis_url);
        redis_client.ping().await.expect("redis ping");

        let pg_pool = setup_pool(&settings.database_url).await.expect("pg pool");
        let verification_email_storage = setup_verification_email_storage(&pg_pool, &settings)
            .await
            .expect("verification email storage");
        let github_webhook_storage = setup_github_webhook_storage(&pg_pool, &settings)
            .await
            .expect("github webhook storage");
        let github_issue_sync_storage = setup_github_issue_sync_storage(&pg_pool, &settings)
            .await
            .expect("github issue sync storage");
        let password_reset_email_storage = setup_password_reset_email_storage(&pg_pool, &settings)
            .await
            .expect("password reset email storage");
        let review_summary_storage = setup_review_summary_storage(&pg_pool, &settings)
            .await
            .expect("setup review summary storage");
        let already_registered_email_storage =
            setup_already_registered_email_storage(&pg_pool, &settings)
                .await
                .expect("already registered email storage");
        let storage = setup_storage().await.expect("storage backend");
        let http_client = create_http_client().expect("http client");
        let webauthn = build_webauthn(&settings).expect("webauthn");

        let mock = MockOAuthHandle::start(MockGitLabUser {
            id: 42_001,
            username: "oauth_test_user".to_string(),
            email: Some(format!("oauth-test-{}@example.com", Uuid::new_v4())),
        })
        .await;

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test listener");
        let addr: SocketAddr = listener.local_addr().expect("local addr");
        let base_url = format!("http://{addr}");

        let oauth_settings = OAuthSettings {
            app_base_url: base_url.clone(),
            encryption_key: TEST_OAUTH_ENCRYPTION_KEY.to_string(),
            default_redirect_path: "/dashboard".to_string(),
            github: None,
            gitlab: None,
            gitlab_selfhosted: Some(ProviderConfig {
                client_id: TEST_OAUTH_CLIENT_ID.to_string(),
                client_secret: TEST_OAUTH_CLIENT_SECRET.to_string(),
            }),
            google: None,
            oidc: None,
        };

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
            review_summary_storage,
            storage,
            drive_config: DriveConfig::from_env(),
            oauth_settings,
            http_client,
            webauthn,
        };

        let session_store = SessionStore::<SessionRedisPool>::new(
            Some(state.redis_client.conn.clone().into()),
            test_session_config(),
        )
        .await
        .expect("session store");

        let (router, _) = routes::create_routes().split_for_parts();
        let router = router
            .with_state(state.clone())
            .layer(SessionLayer::new(session_store));

        let app_router = router.clone();
        tokio::spawn(async move {
            axum::serve(listener, app_router)
                .await
                .expect("serve test app");
        });

        let client = Client::builder()
            .cookie_store(true)
            .redirect(Policy::none())
            .build()
            .expect("reqwest client");

        Self {
            state,
            base_url,
            mock,
            client,
            router,
        }
    }

    pub async fn request(&self, req: Request<Body>) -> TestResponse {
        let response = self
            .router
            .clone()
            .oneshot(req)
            .await
            .expect("router response");
        let status = response.status();
        let set_cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let body = response
            .into_body()
            .collect()
            .await
            .expect("body collect")
            .to_bytes();
        let body = String::from_utf8_lossy(&body).into_owned();
        TestResponse {
            status,
            body,
            set_cookie,
        }
    }

    pub fn instance_url(&self) -> &str {
        &self.mock.base_url
    }

    pub fn set_mock_user(&self, user: MockGitLabUser) {
        self.mock.set_user(user);
    }

    pub async fn insert_user_default(&self) -> TestUser {
        insert_user(&self.state.db, false, false).await
    }

    pub async fn insert_user(&self, is_admin: bool, is_suspended: bool) -> TestUser {
        insert_user(&self.state.db, is_admin, is_suspended).await
    }

    pub async fn insert_oauth_user(&self, email: Option<&str>) -> TestUser {
        let id = Uuid::new_v4();
        let email = email
            .map(str::to_string)
            .unwrap_or_else(|| format!("pw-user-{id}@example.com"));
        let password = "TestPassword123!".to_string();
        let password_hash = create_password_hash(&password).expect("password hash");

        users::ActiveModel {
            id: Set(id),
            username: Set(format!("test_{}", &id.to_string()[..8])),
            bio: Set(Some(String::new())),
            avatar_url: Set(None),
            email: Set(email.clone()),
            email_verified: Set(true),
            password_hash: Set(Some(password_hash)),
            is_admin: Set(false),
            is_suspended: Set(false),
            sessions_revoked_at: Set(None),
            totp_enabled: Set(false),
        }
        .insert(&self.state.db)
        .await
        .expect("insert user");

        TestUser {
            id,
            email,
            password,
        }
    }

    pub async fn insert_passkey_user(
        &self,
        email_verified: bool,
        password: Option<&str>,
    ) -> TestUser {
        insert_passkey_user(&self.state.db, email_verified, password).await
    }

    pub async fn insert_tenant_project(&self, owner_id: Uuid) -> TestTenantProject {
        let tenant_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let suffix = &tenant_id.to_string()[..8];

        tenants::ActiveModel {
            id: Set(tenant_id),
            display_id: Set(format!("gh-{suffix}")),
            name: Set(format!("GitHub Test {suffix}")),
            description: Set(String::new()),
            icon_url: Set(String::new()),
            owner_id: Set(owner_id),
            drive_quota_bytes: Set(None),
            require_2fa: Set(false),
        }
        .insert(&self.state.db)
        .await
        .expect("insert tenant");

        projects::ActiveModel {
            id: Set(project_id),
            name: Set("github-test".into()),
            description: Set(String::new()),
            tenant_id: Set(tenant_id),
            icon_emoji: Set(None),
            icon_url: Set(None),
            // テナントごとに一意なキー。project key 制約 ^[A-Z][A-Z0-9]{1,9}$ を満たす。
            // 全テスト共通の "GHUB" 固定だと、同一 DB を共有する複数テストバイナリ間で衝突する。
            key: Set(format!("P{}", suffix.to_uppercase())),
            is_personal: Set(false),
            personal_owner_id: Set(None),
        }
        .insert(&self.state.db)
        .await
        .expect("insert project");

        TestTenantProject {
            tenant_id,
            project_id,
        }
    }

    pub fn reset_session_client(&mut self) {
        self.client = Client::builder()
            .cookie_store(true)
            .redirect(Policy::none())
            .build()
            .expect("reqwest client");
    }

    pub fn session_client(&self) -> Client {
        self.client.clone()
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn login_session(&mut self, email: &str, password: &str) -> StatusCode {
        let response = self
            .client
            .post(format!("{}/v1/auth/login", self.base_url))
            .json(&serde_json::json!({ "email": email, "password": password }))
            .send()
            .await
            .expect("login request");
        response.status()
    }

    pub async fn login_session_no_content(&mut self, email: &str, password: &str) {
        let status = self.login_session(email, password).await;
        assert_eq!(
            status,
            StatusCode::NO_CONTENT,
            "login expected 204 NO_CONTENT"
        );
    }

    pub async fn enable_2fa(&mut self, user: &TestUser) -> Enabled2fa {
        let status = self.login_session(&user.email, &user.password).await;
        assert_eq!(
            status,
            StatusCode::NO_CONTENT,
            "full login before 2fa setup"
        );

        let setup = self
            .post_json_with_session("/v1/auth/2fa/totp/setup", serde_json::json!({}))
            .await;
        assert_eq!(setup.status(), StatusCode::OK);
        let setup_body: serde_json::Value = setup.json().await.expect("setup json");
        let uri = setup_body["otpauth_uri"].as_str().expect("otpauth_uri");
        let secret = secret_from_otpauth_uri(uri);
        let totp =
            build_totp(&secret, &self.state.settings.totp_issuer, &user.email).expect("build totp");
        let code = totp.generate_current().expect("totp code");

        let verify = self
            .post_json_with_session(
                "/v1/auth/2fa/totp/verify-setup",
                serde_json::json!({ "code": code }),
            )
            .await;
        assert_eq!(verify.status(), StatusCode::OK);
        let body: serde_json::Value = verify.json().await.expect("verify-setup json");
        let codes: Vec<String> = body["recovery_codes"]
            .as_array()
            .expect("recovery_codes array")
            .iter()
            .map(|v| v.as_str().expect("code str").to_string())
            .collect();

        self.reset_session_client();
        Enabled2fa {
            recovery_codes: codes,
        }
    }

    pub async fn login_half_authed(&mut self, user: &TestUser) {
        let response = self
            .client
            .post(format!("{}/v1/auth/login", self.base_url))
            .json(&serde_json::json!({ "email": user.email, "password": user.password }))
            .send()
            .await
            .expect("login");
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "2fa login returns JSON body"
        );
        let body: serde_json::Value = response.json().await.expect("login json");
        assert_eq!(body["requires_2fa"].as_bool(), Some(true));
    }

    pub async fn oauth_start(&self, link: bool) -> Response {
        let mut url = format!(
            "{}/v1/auth/oauth/gitlab_selfhosted?instance_url={}",
            self.base_url,
            urlencoding::encode(self.instance_url())
        );
        if link {
            url.push_str("&redirect_after=/settings");
        }
        self.client
            .get(&url)
            .send()
            .await
            .expect("oauth start request")
    }

    pub async fn follow_oauth_start(&self, start: Response) -> Response {
        assert!(is_redirect(start.status()), "oauth start redirect");
        let authorize_url = start
            .headers()
            .get("location")
            .expect("oauth start location")
            .to_str()
            .expect("location utf8")
            .to_string();

        let to_callback = self
            .client
            .get(authorize_url)
            .send()
            .await
            .expect("follow mock authorize redirect");

        assert!(is_redirect(to_callback.status()), "mock authorize redirect");
        let callback_url = to_callback
            .headers()
            .get("location")
            .expect("callback location")
            .to_str()
            .expect("callback location utf8")
            .to_string();

        self.client
            .get(callback_url)
            .send()
            .await
            .expect("oauth callback request")
    }

    pub async fn get_me(&self) -> Response {
        self.client
            .get(format!("{}/v1/auth/me", self.base_url))
            .send()
            .await
            .expect("me request")
    }

    pub async fn get(&self, path: &str) -> Response {
        self.client
            .get(format!("{}{path}", self.base_url))
            .send()
            .await
            .expect("get request")
    }

    pub async fn get_with_session(&self, path: &str) -> Response {
        self.get(path).await
    }

    pub async fn post_json(&self, path: &str, body: serde_json::Value) -> Response {
        self.client
            .post(format!("{}{path}", self.base_url))
            .json(&body)
            .send()
            .await
            .expect("post request")
    }

    pub async fn post_json_with_session(&self, path: &str, body: serde_json::Value) -> Response {
        self.client
            .post(format!("{}{path}", self.base_url))
            .json(&body)
            .send()
            .await
            .expect("post request")
    }

    pub async fn post_json_with_bearer(
        &self,
        path: &str,
        body: serde_json::Value,
        token: &str,
    ) -> Response {
        self.client
            .post(format!("{}{path}", self.base_url))
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .json(&body)
            .send()
            .await
            .expect("post request")
    }

    pub async fn post_json_with_session_and_headers(
        &self,
        path: &str,
        body: serde_json::Value,
        ip: &str,
        user_agent: &str,
    ) -> Response {
        self.client
            .post(format!("{}{path}", self.base_url))
            .header("x-forwarded-for", ip)
            .header(header::USER_AGENT, user_agent)
            .json(&body)
            .send()
            .await
            .expect("post request")
    }

    pub async fn put_json_with_session(&self, path: &str, body: serde_json::Value) -> Response {
        self.client
            .put(format!("{}{path}", self.base_url))
            .json(&body)
            .send()
            .await
            .expect("put request")
    }

    pub async fn patch_json_with_session(&self, path: &str, body: serde_json::Value) -> Response {
        self.client
            .patch(format!("{}{path}", self.base_url))
            .json(&body)
            .send()
            .await
            .expect("patch request")
    }

    pub async fn delete_with_session(&self, path: &str) -> Response {
        self.client
            .delete(format!("{}{path}", self.base_url))
            .send()
            .await
            .expect("delete request")
    }

    pub async fn get_with_bearer(&self, path: &str, token: &str) -> Response {
        self.client
            .get(format!("{}{path}", self.base_url))
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .send()
            .await
            .expect("bearer request")
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    pub async fn delete_json_with_session(&self, path: &str, body: serde_json::Value) -> Response {
        self.client
            .delete(format!("{}{path}", self.base_url))
            .json(&body)
            .send()
            .await
            .expect("delete request")
    }

    pub async fn count_connections_for_user(&self, user_id: Uuid) -> u64 {
        oauth_connections::Entity::find()
            .filter(oauth_connections::Column::UserId.eq(user_id))
            .all(&self.state.db)
            .await
            .expect("load connections")
            .len() as u64
    }

    pub async fn cleanup_user(&self, user_id: Uuid) {
        let _ = oauth_connections::Entity::delete_many()
            .filter(oauth_connections::Column::UserId.eq(user_id))
            .exec(&self.state.db)
            .await;

        let integrations = github_integrations::Entity::find()
            .filter(github_integrations::Column::CreatedBy.eq(user_id))
            .all(&self.state.db)
            .await
            .expect("list github integrations for cleanup");
        for row in integrations {
            let active: github_integrations::ActiveModel = row.into();
            active
                .delete(&self.state.db)
                .await
                .expect("cleanup github integration");
        }

        let owned_tenants = tenants::Entity::find()
            .filter(tenants::Column::OwnerId.eq(user_id))
            .all(&self.state.db)
            .await
            .expect("list tenants for cleanup");
        for row in owned_tenants {
            let active: tenants::ActiveModel = row.into();
            active.delete(&self.state.db).await.expect("cleanup tenant");
        }

        users::Entity::delete_by_id(user_id)
            .exec(&self.state.db)
            .await
            .expect("cleanup user");
    }
}

pub struct TestResponse {
    pub status: StatusCode,
    pub body: String,
    pub set_cookie: Option<String>,
}

pub async fn insert_user(db: &DatabaseConnection, is_admin: bool, is_suspended: bool) -> TestUser {
    let id = Uuid::new_v4();
    let email = format!("integration-test-{id}@example.com");
    let password = "TestPassword123!".to_string();
    let password_hash = create_password_hash(&password).expect("password hash");

    users::ActiveModel {
        id: Set(id),
        username: Set(format!("test_{}", &id.to_string()[..8])),
        bio: Set(Some(String::new())),
        avatar_url: Set(None),
        email: Set(email.clone()),
        email_verified: Set(true),
        password_hash: Set(Some(password_hash)),
        is_admin: Set(is_admin),
        is_suspended: Set(is_suspended),
        sessions_revoked_at: Set(None),
        totp_enabled: Set(false),
    }
    .insert(db)
    .await
    .expect("insert user");

    TestUser {
        id,
        email,
        password,
    }
}

pub async fn insert_passkey_user(
    db: &DatabaseConnection,
    email_verified: bool,
    password: Option<&str>,
) -> TestUser {
    let id = Uuid::new_v4();
    let email = format!("webauthn-test-{id}@example.com");
    let password = password.map(|p| p.to_string()).unwrap_or_default();
    let password_hash = if password.is_empty() {
        String::new()
    } else {
        create_password_hash(&password).expect("password hash")
    };

    users::ActiveModel {
        id: Set(id),
        username: Set(format!("test_{}", &id.to_string()[..8])),
        bio: Set(Some(String::new())),
        avatar_url: Set(None),
        email: Set(email.clone()),
        email_verified: Set(email_verified),
        password_hash: Set(if password_hash.is_empty() {
            None
        } else {
            Some(password_hash)
        }),
        totp_enabled: Set(false),
        is_admin: Set(false),
        is_suspended: Set(false),
        sessions_revoked_at: Set(None),
    }
    .insert(db)
    .await
    .expect("insert user");

    TestUser {
        id,
        email,
        password,
    }
}

pub fn secret_from_otpauth_uri(uri: &str) -> String {
    let parsed = Url::parse(uri).expect("otpauth uri");
    parsed
        .query_pairs()
        .find(|(k, _)| k == "secret")
        .map(|(_, v)| v.into_owned())
        .expect("secret query param")
}

pub fn current_totp_code(secret: &str, issuer: &str, email: &str) -> String {
    let totp = build_totp(secret, issuer, email).expect("totp");
    totp.generate_current().expect("code")
}

pub async fn insert_personal_token_for_test(
    db: &DatabaseConnection,
    user_id: Uuid,
    tenant_id: Uuid,
    secret: &str,
) -> String {
    use backend::utils::auth::generate_personal_token;
    use sea_orm::Statement;

    let (token, token_hash) = generate_personal_token(secret).expect("generate pat");
    let id = Uuid::new_v4();
    let last_four = token[token.len().saturating_sub(4)..].to_string();
    let stmt = Statement::from_sql_and_values(
        db.get_database_backend(),
        r#"INSERT INTO personal_tokens
            (id, name, token_hash, token_last_four, user_id, tenant_id, revoked, scopes)
            VALUES ($1, $2, $3, $4, $5, $6, false, '["admin:tenant"]'::json)"#,
        vec![
            id.into(),
            "integration-test".into(),
            token_hash.into(),
            last_four.into(),
            user_id.into(),
            tenant_id.into(),
        ],
    );
    db.execute_raw(stmt).await.expect("insert legacy pat");
    token
}

/// アドバイザリロックを握る（`service::drive::lock_tenant_drive` と同じ鍵で押さえる用）。
pub async fn execute_advisory_lock<C: sea_orm::ConnectionTrait>(conn: &C, key: i64) {
    common::db::execute_bound(conn, "SELECT pg_advisory_xact_lock(?)", vec![key.into()])
        .await
        .expect("advisory lock");
}

/// テストの前提を作るための生 SQL。`?` ではなく `$N` を使う（common::db のヘルパー経由）。
pub async fn execute_sql<C: sea_orm::ConnectionTrait>(
    conn: &C,
    sql: &str,
    values: Vec<sea_orm::Value>,
) {
    common::db::execute_bound(conn, sql, values)
        .await
        .expect("execute sql");
}

pub async fn insert_tenant(db: &DatabaseConnection, owner_id: Uuid) -> Uuid {
    let id = Uuid::new_v4();
    tenants::ActiveModel {
        id: Set(id),
        display_id: Set(format!("t-{}", &id.to_string()[..8])),
        name: Set("Test Tenant".into()),
        description: Set(String::new()),
        icon_url: Set(String::new()),
        owner_id: Set(owner_id),
        drive_quota_bytes: Set(None),
        require_2fa: Set(false),
    }
    .insert(db)
    .await
    .expect("insert tenant");
    id
}

/// #568: プロジェクトメンバーはテナントメンバーの絞り込みなので、
/// テストが `project_members` へ直接 INSERT する場合もテナントメンバーを先に用意する。
/// 既に居る場合・テナントオーナーの場合は何もしない。
pub async fn ensure_tenant_member_for_project(
    db: &DatabaseConnection,
    project_id: Uuid,
    user_id: Uuid,
) {
    let project = projects::Entity::find_by_id(project_id)
        .one(db)
        .await
        .expect("find project")
        .expect("project exists");
    let tenant = tenants::Entity::find_by_id(project.tenant_id)
        .one(db)
        .await
        .expect("find tenant")
        .expect("tenant exists");
    if tenant.owner_id == user_id {
        return;
    }
    let existing = tenant_members::Entity::find()
        .filter(tenant_members::Column::TenantId.eq(project.tenant_id))
        .filter(tenant_members::Column::UserId.eq(user_id))
        .one(db)
        .await
        .expect("find tenant member");
    if existing.is_some() {
        return;
    }
    tenant_members::ActiveModel {
        id: Set(Uuid::new_v4()),
        tenant_id: Set(project.tenant_id),
        user_id: Set(user_id),
        role: Set(entity::tenant_members::TenantRole::Member),
    }
    .insert(db)
    .await
    .expect("insert tenant member");
}
