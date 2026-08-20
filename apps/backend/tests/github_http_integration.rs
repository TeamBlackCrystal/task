mod common;

use axum::http::StatusCode;
use backend::utils::github::install_state::{self as github_oauth_state, GithubOAuthStatePayload};
use common::{TestApp, TestTenantProject};
use entity::{github_integrations, projects, tenants};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;
use wiremock::matchers::{header, method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn mount_github_api_mocks(server: &MockServer) {
    // installation id の帯でトークンを変え、リポジトリ一覧のモックを出し分ける
    // （単一リポジトリ / 複数リポジトリのインストールを同じ MockServer で共存させるため）。
    Mock::given(method("POST"))
        .and(path_regex(r"^/app/installations/\d+/access_tokens$"))
        .respond_with(|req: &wiremock::Request| {
            let id = installation_id_from_url(&req.url);
            let token = if id >= NO_REPO_ID_BASE {
                "ghs_no_repo_token"
            } else if id >= MULTI_REPO_ID_BASE {
                "ghs_multi_repo_token"
            } else {
                "ghs_test_installation_token"
            };
            ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "token": token,
                "expires_at": "2030-01-01T00:00:00Z"
            }))
        })
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path_regex(r"^/app/installations/\d+$"))
        .respond_with(|req: &wiremock::Request| {
            let installation_id = installation_id_from_url(&req.url);
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
        .and(header(
            "authorization",
            "Bearer ghs_test_installation_token",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "repositories": [{
                "full_name": "acme/backend",
                "owner": { "login": "acme" }
            }]
        })))
        .mount(server)
        .await;

    let many: Vec<serde_json::Value> = (0..30)
        .map(|i| {
            serde_json::json!({
                "full_name": format!("acme/repo-{i}"),
                "owner": { "login": "acme" }
            })
        })
        .collect();
    Mock::given(method("GET"))
        .and(path("/installation/repositories"))
        .and(header("authorization", "Bearer ghs_no_repo_token"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "repositories": [] })),
        )
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/installation/repositories"))
        .and(header("authorization", "Bearer ghs_multi_repo_token"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "repositories": many })),
        )
        .mount(server)
        .await;

    Mock::given(method("DELETE"))
        .and(path_regex(r"^/app/installations/\d+$"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1..)
        .mount(server)
        .await;
}

/// この値以上の installation id は「複数リポジトリが見えるインストール」として扱う。
const MULTI_REPO_ID_BASE: i64 = 1_500_000_000_000;
/// この値以上は「1 件も見えないインストール」。
const NO_REPO_ID_BASE: i64 = 2_500_000_000_000;

fn installation_id_from_url(url: &url::Url) -> i64 {
    url.path_segments()
        .and_then(|mut segments| segments.find_map(|segment| segment.parse::<i64>().ok()))
        .unwrap_or(0)
}

fn unique_installation_id() -> i64 {
    300_000_000_000_i64 + (Uuid::new_v4().as_u128() % 900_000_000_000) as i64
}

fn unique_multi_repo_installation_id() -> i64 {
    MULTI_REPO_ID_BASE + (Uuid::new_v4().as_u128() % 900_000_000_000) as i64
}

fn unique_no_repo_installation_id() -> i64 {
    NO_REPO_ID_BASE + (Uuid::new_v4().as_u128() % 900_000_000_000) as i64
}

fn repositories_path(tp: &TestTenantProject, select_token: &str) -> String {
    format!(
        "/v1/tenants/{}/projects/{}/github/repositories?select_token={select_token}",
        tp.tenant_id, tp.project_id
    )
}

fn connect_path(tp: &TestTenantProject) -> String {
    format!(
        "/v1/tenants/{}/projects/{}/github/connect",
        tp.tenant_id, tp.project_id
    )
}

fn select_token_from_location(location: &str) -> String {
    url::Url::parse(location)
        .expect("redirect location")
        .query_pairs()
        .find(|(key, _)| key == "github_select")
        .map(|(_, value)| value.into_owned())
        .expect("github_select query param")
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

        let delete = app.delete_with_session(&integration_path(&tp)).await;
        assert_eq!(delete.status(), StatusCode::NO_CONTENT);

        let remaining = github_integrations::Entity::find()
            .filter(github_integrations::Column::ProjectId.eq(tp.project_id))
            .one(&app.state.db)
            .await
            .expect("query integration");
        assert!(remaining.is_none());

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

    // 7. 複数リポジトリが見えるインストール — 選択トークン経由で 1 件選んで連携する（#594）
    {
        let user = app.insert_user(false, false).await;
        let tp = app.insert_tenant_project(user.id).await;
        app.login_session(&user.email, &user.password).await;

        let state_token = get_install_state(&app, &tp).await;
        let installation_id = unique_multi_repo_installation_id();
        let response = app
            .get_with_session(&callback_path(&state_token, installation_id))
            .await;
        let status = response.status();
        assert!(
            status == StatusCode::FOUND || status == StatusCode::TEMPORARY_REDIRECT,
            "callback should redirect, got {status}"
        );
        let location = response
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
            .expect("location header");

        // 修正前は 400（"select one explicitly"）で、ここまで到達しなかった。
        let select_token = select_token_from_location(&location);

        // 選択が済むまで連携レコードは作らない
        let pending = github_integrations::Entity::find()
            .filter(github_integrations::Column::ProjectId.eq(tp.project_id))
            .one(&app.state.db)
            .await
            .expect("query integration");
        assert!(pending.is_none(), "integration must wait for selection");

        // 一覧が取れる（トークンは消費されない）
        let list = app
            .get_with_session(&repositories_path(&tp, &select_token))
            .await;
        assert_eq!(list.status(), StatusCode::OK);
        let body: serde_json::Value = list.json().await.expect("repositories json");
        assert_eq!(body["repositories"].as_array().unwrap().len(), 30);

        // 別ユーザーは同じ選択トークンを使えない（403）
        let owner_email = user.email.clone();
        let owner_password = user.password.clone();
        app.reset_session_client();
        let other = app.insert_user(false, false).await;
        app.login_session(&other.email, &other.password).await;
        let stolen_list = app
            .get_with_session(&repositories_path(&tp, &select_token))
            .await;
        assert_eq!(stolen_list.status(), StatusCode::FORBIDDEN);
        let stolen_connect = app
            .post_json_with_session(
                &connect_path(&tp),
                serde_json::json!({
                    "select_token": select_token,
                    "repo_owner": "acme",
                    "repo_name": "repo-1"
                }),
            )
            .await;
        assert_eq!(stolen_connect.status(), StatusCode::FORBIDDEN);
        app.cleanup_user(other.id).await;
        app.reset_session_client();
        app.login_session(&owner_email, &owner_password).await;

        // 同じユーザーでも、トークンに束縛されていない別プロジェクトには使えない（400）
        let other_project = app.insert_tenant_project(user.id).await;
        let wrong_project = app
            .post_json_with_session(
                &connect_path(&other_project),
                serde_json::json!({
                    "select_token": select_token,
                    "repo_owner": "acme",
                    "repo_name": "repo-1"
                }),
            )
            .await;
        assert_eq!(wrong_project.status(), StatusCode::BAD_REQUEST);

        // installation の可視範囲にないリポジトリは拒否する（このときトークンは残す）
        let rejected = app
            .post_json_with_session(
                &connect_path(&tp),
                serde_json::json!({
                    "select_token": select_token,
                    "repo_owner": "attacker",
                    "repo_name": "private"
                }),
            )
            .await;
        assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);

        let connect = app
            .post_json_with_session(
                &connect_path(&tp),
                serde_json::json!({
                    "select_token": select_token,
                    "repo_owner": "acme",
                    "repo_name": "repo-7"
                }),
            )
            .await;
        assert_eq!(connect.status(), StatusCode::NO_CONTENT);

        let row = github_integrations::Entity::find()
            .filter(github_integrations::Column::ProjectId.eq(tp.project_id))
            .one(&app.state.db)
            .await
            .expect("query integration")
            .expect("integration row");
        assert_eq!(row.installation_id, installation_id);
        assert_eq!(row.repo_owner, "acme");
        assert_eq!(row.repo_name, "repo-7");

        // 確定後のトークンは使い捨て
        let reused = app
            .post_json_with_session(
                &connect_path(&tp),
                serde_json::json!({
                    "select_token": select_token,
                    "repo_owner": "acme",
                    "repo_name": "repo-8"
                }),
            )
            .await;
        assert_eq!(reused.status(), StatusCode::BAD_REQUEST);

        app.cleanup_user(user.id).await;
        app.reset_session_client();
    }

    // 8. 1 件も見えないインストールは選択画面に入れず 400（#594 レビュー指摘）
    {
        let user = app.insert_user(false, false).await;
        let tp = app.insert_tenant_project(user.id).await;
        app.login_session(&user.email, &user.password).await;

        let state_token = get_install_state(&app, &tp).await;
        let response = app
            .get_with_session(&callback_path(
                &state_token,
                unique_no_repo_installation_id(),
            ))
            .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let row = github_integrations::Entity::find()
            .filter(github_integrations::Column::ProjectId.eq(tp.project_id))
            .one(&app.state.db)
            .await
            .expect("query integration");
        assert!(row.is_none());

        app.cleanup_user(user.id).await;
        app.reset_session_client();
    }

    // 9. 選択を放棄しても同じインストールへ戻れる／連携解除で束縛が残らない（#594 レビュー指摘）
    {
        let user = app.insert_user(false, false).await;
        let tp = app.insert_tenant_project(user.id).await;
        app.login_session(&user.email, &user.password).await;

        // 選択画面まで進んで放棄する
        let state_token = get_install_state(&app, &tp).await;
        let installation_id = unique_multi_repo_installation_id();
        let abandoned = app
            .get_with_session(&callback_path(&state_token, installation_id))
            .await;
        assert!(
            abandoned.status() == StatusCode::FOUND
                || abandoned.status() == StatusCode::TEMPORARY_REDIRECT
        );

        // 再訪: 同じインストールで戻ってこられる（新規扱いの鮮度チェックで弾かれない）
        let retry_state = get_install_state(&app, &tp).await;
        let retry = app
            .get_with_session(&callback_path(&retry_state, installation_id))
            .await;
        assert!(
            retry.status() == StatusCode::FOUND || retry.status() == StatusCode::TEMPORARY_REDIRECT,
            "abandoned installation should be reusable, got {}",
            retry.status()
        );
        let select_token = select_token_from_location(
            retry
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .expect("location header"),
        );

        let connect = app
            .post_json_with_session(
                &connect_path(&tp),
                serde_json::json!({
                    "select_token": select_token,
                    "repo_owner": "acme",
                    "repo_name": "repo-3"
                }),
            )
            .await;
        assert_eq!(connect.status(), StatusCode::NO_CONTENT);

        // 解除したあとは別のインストールで連携し直せる（古い束縛が残っていない）
        let delete = app.delete_with_session(&integration_path(&tp)).await;
        assert_eq!(delete.status(), StatusCode::NO_CONTENT);

        let new_state = get_install_state(&app, &tp).await;
        let reinstalled = app
            .get_with_session(&callback_path(&new_state, unique_installation_id()))
            .await;
        assert!(
            reinstalled.status() == StatusCode::FOUND
                || reinstalled.status() == StatusCode::TEMPORARY_REDIRECT,
            "reinstall with a new installation should be accepted, got {}",
            reinstalled.status()
        );

        app.cleanup_user(user.id).await;
        app.reset_session_client();
    }
}
