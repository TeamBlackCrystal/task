mod common;

use axum::http::StatusCode;
use common::{MockGitLabUser, TestApp, is_redirect};
use entity::{oauth_connections, users};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use url::Url;

#[tokio::test]
async fn oauth_start_callback_flow_issues_session() {
    let app = TestApp::new().await;
    let unique = uuid::Uuid::new_v4();
    app.set_mock_user(MockGitLabUser {
        id: 100_001,
        username: format!("oauth_user_{unique}"),
        email: Some(format!("oauth-flow-{unique}@example.com")),
    });

    let start = app.oauth_start(false).await;
    assert!(is_redirect(start.status()), "oauth start redirect");
    let authorize_url = start
        .headers()
        .get("location")
        .expect("authorize redirect")
        .to_str()
        .expect("authorize url");
    let parsed = Url::parse(authorize_url).expect("authorize url parse");
    let state = parsed
        .query_pairs()
        .find(|(k, _)| k == "state")
        .map(|(_, v)| v.to_string())
        .expect("state query param");
    assert!(!state.is_empty(), "oauth state must be generated");

    let callback = app.follow_oauth_start(start).await;
    assert!(is_redirect(callback.status()), "oauth callback redirect");
    let frontend = callback
        .headers()
        .get("location")
        .expect("frontend redirect")
        .to_str()
        .expect("frontend url");
    assert!(
        frontend.starts_with("http://localhost:3000/dashboard"),
        "unexpected frontend redirect: {frontend}"
    );

    let me = app.get_me().await;
    assert_eq!(me.status(), StatusCode::OK);
    let body: serde_json::Value = me.json().await.expect("me json");
    assert_eq!(
        body["email"].as_str(),
        Some(format!("oauth-flow-{unique}@example.com").as_str())
    );

    let user_id: uuid::Uuid = body["id"]
        .as_str()
        .expect("user id")
        .parse()
        .expect("uuid parse");
    assert_eq!(app.count_connections_for_user(user_id).await, 1);

    app.cleanup_user(user_id).await;
}

#[tokio::test]
async fn oauth_callback_rejects_state_mismatch() {
    let app = TestApp::new().await;
    let start = app.oauth_start(false).await;
    assert!(is_redirect(start.status()), "oauth start redirect");

    let response = app
        .get(
            "/v1/auth/oauth/gitlab_selfhosted/callback?code=mock-auth-code&state=wrong-state-value",
        )
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.text().await.expect("body");
    assert!(body.contains("invalid-oauth-state"));
}

#[tokio::test]
async fn oauth_disconnect_removes_connection() {
    let mut app = TestApp::new().await;
    let unique = uuid::Uuid::new_v4();
    let user = app.insert_oauth_user(None).await;
    app.login_session(&user.email, &user.password).await;

    app.set_mock_user(MockGitLabUser {
        id: 200_001,
        username: format!("linked_{unique}"),
        email: Some(format!("linked-{unique}@example.com")),
    });

    let start = app.oauth_start(true).await;
    let callback = app.follow_oauth_start(start).await;
    assert!(is_redirect(callback.status()), "oauth callback redirect");

    assert_eq!(app.count_connections_for_user(user.id).await, 1);

    let disconnect_path = format!(
        "/v1/auth/oauth/connections/gitlab_selfhosted?instance_url={}",
        urlencoding::encode(app.instance_url())
    );
    let disconnect = app.delete_with_session(&disconnect_path).await;
    assert_eq!(disconnect.status(), StatusCode::NO_CONTENT);
    assert_eq!(app.count_connections_for_user(user.id).await, 0);

    app.cleanup_user(user.id).await;
}

#[tokio::test]
async fn oauth_disconnect_not_found_for_unlinked_user() {
    let mut app = TestApp::new().await;
    let user = app.insert_oauth_user(None).await;
    app.login_session(&user.email, &user.password).await;

    let disconnect_path = format!(
        "/v1/auth/oauth/connections/gitlab_selfhosted?instance_url={}",
        urlencoding::encode(app.instance_url())
    );
    let response = app.delete_with_session(&disconnect_path).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = response.text().await.expect("body");
    assert!(body.contains("oauth-connection-not-found"));

    app.cleanup_user(user.id).await;
}

#[tokio::test]
async fn oauth_callback_returns_email_conflict() {
    let app = TestApp::new().await;
    let unique = uuid::Uuid::new_v4();
    let email = format!("conflict-{unique}@example.com");
    let existing = app.insert_oauth_user(Some(&email)).await;

    app.set_mock_user(MockGitLabUser {
        id: 300_001,
        username: format!("conflict_{unique}"),
        email: Some(email.clone()),
    });

    let start = app.oauth_start(false).await;
    let callback = app.follow_oauth_start(start).await;
    assert_eq!(callback.status(), StatusCode::CONFLICT);
    let body = callback.text().await.expect("body");
    assert!(body.contains("oauth-email-conflict"));

    let still_one = users::Entity::find()
        .filter(users::Column::Email.eq(email))
        .all(&app.state.db)
        .await
        .expect("query users")
        .len();
    assert_eq!(still_one, 1);

    let connections = oauth_connections::Entity::find()
        .filter(oauth_connections::Column::ProviderUserId.eq("300001"))
        .all(&app.state.db)
        .await
        .expect("query connections");
    assert!(connections.is_empty());

    app.cleanup_user(existing.id).await;
}

#[tokio::test]
async fn oauth_disconnect_last_auth_method_returns_403() {
    let app = TestApp::new().await;
    let unique = uuid::Uuid::new_v4();
    app.set_mock_user(MockGitLabUser {
        id: 400_001,
        username: format!("oauth_only_{unique}"),
        email: Some(format!("oauth-only-{unique}@example.com")),
    });

    let start = app.oauth_start(false).await;
    let callback = app.follow_oauth_start(start).await;
    assert!(is_redirect(callback.status()), "oauth callback redirect");

    let me = app.get_me().await;
    assert_eq!(me.status(), StatusCode::OK);
    let body: serde_json::Value = me.json().await.expect("me json");
    let user_id: uuid::Uuid = body["id"]
        .as_str()
        .expect("user id")
        .parse()
        .expect("uuid parse");
    assert_eq!(app.count_connections_for_user(user_id).await, 1);

    let disconnect_path = format!(
        "/v1/auth/oauth/connections/gitlab_selfhosted?instance_url={}",
        urlencoding::encode(app.instance_url())
    );
    let response = app.delete_with_session(&disconnect_path).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = response.text().await.expect("body");
    assert!(body.contains("oauth-last-auth-method"));

    app.cleanup_user(user_id).await;
}

#[tokio::test]
async fn oauth_callback_provider_error_redirects_with_oauth_error() {
    let app = TestApp::new().await;

    let response = app
        .get("/v1/auth/oauth/gitlab_selfhosted/callback?error=access_denied")
        .await;
    assert!(is_redirect(response.status()), "provider error redirect");
    let location = response
        .headers()
        .get("location")
        .expect("redirect location")
        .to_str()
        .expect("redirect location utf8");
    assert!(
        location.contains("oauth_error=authorization_failed"),
        "unexpected redirect location: {location}"
    );
}

#[tokio::test]
async fn oauth_providers_lists_only_configured() {
    let app = TestApp::new().await;

    let response = app.get("/v1/auth/oauth/providers").await;
    assert_eq!(response.status(), StatusCode::OK);

    let body: serde_json::Value = response.json().await.expect("providers json");
    let providers = body["providers"].as_array().expect("providers array");
    let slugs: Vec<&str> = providers
        .iter()
        .map(|p| p["provider"].as_str().expect("provider slug"))
        .collect();

    // テストハーネスは gitlab_selfhosted のみ設定済み → 有効なものだけ返る（出し分けの証明）。
    assert!(
        slugs.contains(&"gitlab_selfhosted"),
        "configured provider must be listed: {slugs:?}"
    );
    assert!(
        !slugs.contains(&"github") && !slugs.contains(&"gitlab") && !slugs.contains(&"google"),
        "unconfigured providers must not be listed: {slugs:?}"
    );

    // self-hosted は instance_url 入力が必要というメタが付く。
    let selfhosted = providers
        .iter()
        .find(|p| p["provider"] == "gitlab_selfhosted")
        .expect("gitlab_selfhosted present");
    assert_eq!(
        selfhosted["requires_instance_url"].as_bool(),
        Some(true),
        "gitlab_selfhosted must require instance_url"
    );
}

#[tokio::test]
async fn oauth_provider_error_uses_error_redirect_after() {
    let app = TestApp::new().await;

    // 成功用 redirect_after=/ とは別に error_redirect_after=/signin を指定して開始する。
    let start = app
        .get(&format!(
            "/v1/auth/oauth/gitlab_selfhosted?instance_url={}&redirect_after=/&error_redirect_after=/signin",
            urlencoding::encode(app.instance_url())
        ))
        .await;
    assert!(is_redirect(start.status()), "oauth start redirect");
    let authorize_url = start
        .headers()
        .get("location")
        .expect("authorize redirect")
        .to_str()
        .expect("authorize url");
    let state = Url::parse(authorize_url)
        .expect("authorize url parse")
        .query_pairs()
        .find(|(k, _)| k == "state")
        .map(|(_, v)| v.to_string())
        .expect("state query param");

    // プロバイダーがエラーを返すと、成功用(/)ではなく error_redirect_after(/signin)へ
    // oauth_error 付きで戻る（OAuth ボタンのあるページでエラーを表示させるため）。
    let callback = app
        .get(&format!(
            "/v1/auth/oauth/gitlab_selfhosted/callback?error=access_denied&state={state}"
        ))
        .await;
    assert!(is_redirect(callback.status()), "callback redirect");
    let location = callback
        .headers()
        .get("location")
        .expect("callback location")
        .to_str()
        .expect("callback location utf8");
    let parsed = Url::parse(location).expect("callback location parse");
    assert_eq!(
        parsed.path(),
        "/signin",
        "provider error must return to error_redirect_after, got: {location}"
    );
    assert!(
        parsed
            .query_pairs()
            .any(|(k, v)| k == "oauth_error" && v == "authorization_failed"),
        "missing oauth_error marker: {location}"
    );
}
