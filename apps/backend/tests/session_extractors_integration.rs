mod common;

use axum::http::StatusCode;
use common::TestApp;
use uuid::Uuid;

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

/// Bearer による CSRF Origin 検査の免除を、Cookie セッション認証へ流用できない。
#[tokio::test]
async fn current_user_state_change_rejects_cross_origin_bearer_with_session_cookie() {
    let mut app = TestApp::new().await;

    let user = app.insert_user(false, false).await;
    app.reset_session_client();
    app.login_session_no_content(&user.email, &user.password)
        .await;

    // Bearer があるため CSRF ミドルウェアは許可外 Origin を検査せず通す。
    // その後 CurrentUser が Cookie セッションへのフォールバックを拒否しなければ、
    // 状態変更ハンドラまで到達してしまう。
    let rejected = app
        .client()
        .patch(format!(
            "{}/v1/auth/passkeys/{}",
            app.base_url(),
            Uuid::new_v4()
        ))
        .header(reqwest::header::ORIGIN, "https://attacker.example")
        .header(reqwest::header::AUTHORIZATION, "Bearer not-a-real-token")
        .json(&serde_json::json!({ "name": "hijacked" }))
        .send()
        .await
        .expect("cross-origin passkey rename with bearer and session cookie");
    assert_eq!(
        rejected.status(),
        StatusCode::UNAUTHORIZED,
        "CSRF 免除された Bearer 付き状態変更でもセッション認証へフォールバックしない"
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
