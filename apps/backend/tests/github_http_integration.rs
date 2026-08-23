mod common;

use axum::http::StatusCode;
use backend::utils::github::install_state::{self as github_oauth_state, GithubOAuthStatePayload};
use common::{TestApp, TestTenantProject};
use entity::{github_integrations, projects, tenants};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn mount_github_api_mocks(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path_regex(r"^/app/installations/\d+/access_tokens$"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "token": "ghs_test_installation_token",
            "expires_at": "2030-01-01T00:00:00Z"
        })))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path_regex(r"^/app/installations/\d+$"))
        .respond_with(|req: &wiremock::Request| {
            let installation_id = req
                .url
                .path_segments()
                .and_then(|mut segments| segments.next_back())
                .and_then(|id| id.parse::<i64>().ok())
                .unwrap_or(0);
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": installation_id,
                "account": { "login": "acme" },
                "created_at": chrono::Utc::now().to_rfc3339(),
            }))
        })
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/installation/repositories"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "repositories": [{
                "full_name": "acme/backend",
                "owner": { "login": "acme" }
            }]
        })))
        .mount(server)
        .await;

    Mock::given(method("DELETE"))
        .and(path_regex(r"^/app/installations/\d+$"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(server)
        .await;
}

fn unique_installation_id() -> i64 {
    300_000_000_000_i64 + (Uuid::new_v4().as_u128() % 900_000_000_000) as i64
}

fn install_path(tp: &TestTenantProject) -> String {
    format!(
        "/v1/tenants/{}/projects/{}/github/install",
        tp.tenant_id, tp.project_id
    )
}

fn callback_path(state: &str, installation_id: i64) -> String {
    format!("/v1/github/callback?state={state}&installation_id={installation_id}")
}

fn state_from_install_url(url: &str) -> String {
    url::Url::parse(url)
        .expect("install url")
        .query_pairs()
        .find(|(key, _)| key == "state")
        .map(|(_, value)| value.into_owned())
        .expect("state query param in install url")
}

/// GET /install を呼び、200 OK + JSON body から state トークンを取り出す。
async fn get_install_state(app: &TestApp, tp: &TestTenantProject) -> String {
    let response = app.get_with_session(&install_path(tp)).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "install should return 200"
    );
    let body: serde_json::Value = response.json().await.expect("install json body");
    let url = body["url"].as_str().expect("install url field");
    assert!(url.contains("github.com/apps/task-app/installations/new"));
    assert!(url.contains("state="));
    state_from_install_url(url)
}

fn integration_path(tp: &TestTenantProject) -> String {
    format!(
        "/v1/tenants/{}/projects/{}/github/integration",
        tp.tenant_id, tp.project_id
    )
}

// serial: GITHUB_API_BASE_URL を OnceLock でキャッシュするため、
// 並列実行すると別テストが先にキャッシュした URL が使われる競合が起きる。
#[serial_test::serial]
#[tokio::test]
async fn github_http_integration_suite() {
    let mock_server = MockServer::start().await;
    // SAFETY: シングルスレッドの初期化前に set_var するため safe。
    // serial アトリビュートにより他テストとの並列実行を防いでいる。
    unsafe {
        std::env::set_var("GITHUB_API_BASE_URL", mock_server.uri());
    }
    mount_github_api_mocks(&mock_server).await;

    let mut app = TestApp::new_with_github().await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // 1. GET /install — GitHub インストール URL を JSON で返す
    {
        let user = app.insert_user(false, false).await;
        let tp = app.insert_tenant_project(user.id).await;
        app.login_session(&user.email, &user.password).await;

        let _ = get_install_state(&app, &tp).await;

        app.cleanup_user(user.id).await;
        app.reset_session_client();
    }

    // 2. GET /callback 正常系 — /install の state を /callback に渡して DB に integration 作成
    {
        let user = app.insert_user(false, false).await;
        let tp = app.insert_tenant_project(user.id).await;
        app.login_session(&user.email, &user.password).await;

        let state_token = get_install_state(&app, &tp).await;

        let installation_id = unique_installation_id();
        let response = app
            .get_with_session(&callback_path(&state_token, installation_id))
            .await;
        let status = response.status();
        let location = response
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let body = response.text().await.unwrap_or_default();
        assert!(
            status == StatusCode::FOUND || status == StatusCode::TEMPORARY_REDIRECT,
            "callback failed: status={status} body={body}"
        );

        // 回帰テスト: 戻り先は frontend に実在するルート
        // （display_id / プロジェクトキー基準 + section クエリ）であること。
        // 修正前は /tenants/{uuid}/projects/{uuid}/settings/github（実在しない）だった。
        let tenant = tenants::Entity::find_by_id(tp.tenant_id)
            .one(&app.state.db)
            .await
            .expect("query tenant")
            .expect("tenant row");
        let project = projects::Entity::find_by_id(tp.project_id)
            .one(&app.state.db)
            .await
            .expect("query project")
            .expect("project row");
        let location = location.expect("location header");
        assert!(
            location.ends_with(&format!(
                "/{}/projects/{}/settings?section=integrations",
                tenant.display_id, project.key
            )),
            "unexpected redirect location: {location}"
        );
        assert!(
            !location.contains("/tenants/"),
            "redirect still uses non-existent uuid route: {location}"
        );

        let row = github_integrations::Entity::find()
            .filter(github_integrations::Column::ProjectId.eq(tp.project_id))
            .one(&app.state.db)
            .await
            .expect("query integration")
            .expect("integration row");
        assert_eq!(row.installation_id, installation_id);
        assert_eq!(row.repo_owner, "acme");
        assert_eq!(row.repo_name, "backend");

        app.cleanup_user(user.id).await;
        app.reset_session_client();
    }

    // 3. GET /callback — 無効な state → 400
    {
        let user = app.insert_user(false, false).await;
        app.login_session(&user.email, &user.password).await;

        let response = app
            .get_with_session(&callback_path(
                "nonexistent-state-token",
                unique_installation_id(),
            ))
            .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        app.cleanup_user(user.id).await;
        app.reset_session_client();
    }

    // 4. GET /callback — installation_id が state と不一致 → 400
    {
        let user = app.insert_user(false, false).await;
        let tp = app.insert_tenant_project(user.id).await;
        app.login_session(&user.email, &user.password).await;

        let bound_id = unique_installation_id();
        let state_token = github_oauth_state::new_state_token();
        github_oauth_state::store_state(
            &app.state.redis_client,
            &state_token,
            &GithubOAuthStatePayload {
                tenant_id: tp.tenant_id,
                project_id: tp.project_id,
                user_id: user.id,
                installation_id: Some(bound_id),
            },
        )
        .await
        .expect("store oauth state");

        let response = app
            .get_with_session(&callback_path(&state_token, bound_id + 1))
            .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        app.cleanup_user(user.id).await;
        app.reset_session_client();
    }

    // 5. DELETE /integration 正常系
    {
        let user = app.insert_user(false, false).await;
        let tp = app.insert_tenant_project(user.id).await;
        app.login_session(&user.email, &user.password).await;

        let state_token = get_install_state(&app, &tp).await;

        let installation_id = unique_installation_id();
        let callback = app
            .get_with_session(&callback_path(&state_token, installation_id))
            .await;
        let cb_status = callback.status();
        assert!(
            cb_status == StatusCode::FOUND || cb_status == StatusCode::TEMPORARY_REDIRECT,
            "callback status={cb_status}"
        );

        let lock_token =
            backend::utils::github::try_acquire_import_slot(&app.state.redis_client, tp.project_id)
                .await
                .expect("acquire import slot before disconnect")
                .expect("import slot should be available");

        let delete = app.delete_with_session(&integration_path(&tp)).await;
        assert_eq!(delete.status(), StatusCode::NO_CONTENT);

        let remaining = github_integrations::Entity::find()
            .filter(github_integrations::Column::ProjectId.eq(tp.project_id))
            .one(&app.state.db)
            .await
            .expect("query integration");
        assert!(remaining.is_none());

        let reacquired =
            backend::utils::github::try_acquire_import_slot(&app.state.redis_client, tp.project_id)
                .await
                .expect("reacquire import slot after disconnect");
        assert!(reacquired.is_some(), "連携解除時に取り込み枠を破棄する");
        assert!(
            !backend::utils::github::release_import_slot(
                &app.state.redis_client,
                tp.project_id,
                &lock_token,
            )
            .await
            .expect("release disconnected import slot"),
            "解除前のトークンは無効になっている"
        );
        let reacquired = reacquired.expect("reacquired import slot token");
        backend::utils::github::release_import_slot(
            &app.state.redis_client,
            tp.project_id,
            &reacquired,
        )
        .await
        .expect("release reacquired import slot");

        app.cleanup_user(user.id).await;
        app.reset_session_client();
    }

    // 6. DELETE /integration — 未連携 → 404
    {
        let user = app.insert_user(false, false).await;
        let tp = app.insert_tenant_project(user.id).await;
        app.login_session(&user.email, &user.password).await;

        let response = app.delete_with_session(&integration_path(&tp)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        app.cleanup_user(user.id).await;
    }
}
