mod common;

use axum::http::StatusCode;
use common::TestApp;

/// セッション専用エンドポイントは `Authorization: Bearer` 付きのリクエストを拒否する。
///
/// CSRF の Origin 検査は Bearer 付きリクエストを PAT 経路とみなして素通しする。
/// Cookie セッションで認証するエクストラクタが Bearer を無視すると、Origin 検査を
/// 迂回した状態でセッション認証が通ってしまうため、Bearer が付いていたら拒否する。
#[tokio::test]
async fn current_user_endpoint_rejects_bearer_header() {
    let mut app = TestApp::new().await;

    let user = app.insert_user(false, false).await;
    app.reset_session_client();
    app.login_session_no_content(&user.email, &user.password)
        .await;

    // 対照: Bearer なしのセッションなら取得できる。
    let allowed = app.get_with_session("/v1/auth/me").await;
    assert_eq!(
        allowed.status(),
        StatusCode::OK,
        "セッションのみなら取得できる"
    );

    // 同じセッション Cookie に Bearer を足すと拒否される。
    let rejected = app
        .client()
        .get(format!("{}/v1/auth/me", app.base_url()))
        .header(reqwest::header::AUTHORIZATION, "Bearer not-a-real-token")
        .send()
        .await
        .expect("me request with bearer");
    assert_eq!(
        rejected.status(),
        StatusCode::UNAUTHORIZED,
        "Bearer 付きのセッションリクエストは拒否される"
    );

    app.cleanup_user(user.id).await;
}

/// 管理者専用エンドポイントも同様に `Authorization: Bearer` 付きを拒否する。
#[tokio::test]
async fn admin_user_endpoint_rejects_bearer_header() {
    let mut app = TestApp::new().await;

    let admin = app.insert_user(true, false).await;
    app.reset_session_client();
    app.login_session_no_content(&admin.email, &admin.password)
        .await;

    // 対照: Bearer なしの管理者セッションなら一覧を取得できる。
    let allowed = app.get_with_session("/v1/admin/users").await;
    assert_eq!(
        allowed.status(),
        StatusCode::OK,
        "管理者セッションなら取得できる"
    );

    let rejected = app
        .client()
        .get(format!("{}/v1/admin/users", app.base_url()))
        .header(reqwest::header::AUTHORIZATION, "Bearer not-a-real-token")
        .send()
        .await
        .expect("admin users request with bearer");
    assert_eq!(
        rejected.status(),
        StatusCode::UNAUTHORIZED,
        "Bearer 付きの管理者リクエストは拒否される"
    );

    app.cleanup_user(admin.id).await;
}
