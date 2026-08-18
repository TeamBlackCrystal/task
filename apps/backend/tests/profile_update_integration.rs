mod common;

use axum::http::StatusCode;
use common::TestApp;

async fn me_json(app: &TestApp) -> serde_json::Value {
    app.get_me()
        .await
        .json::<serde_json::Value>()
        .await
        .expect("me body")
}

/// 送った項目だけが更新され、GET /me にも反映される。
#[tokio::test]
async fn updates_editable_fields_and_persists() {
    let mut app = TestApp::new().await;

    let user = app.insert_user(false, false).await;
    app.reset_session_client();
    app.login_session_no_content(&user.email, &user.password)
        .await;

    let res = app
        .patch_json_with_session(
            "/v1/auth/me",
            serde_json::json!({
                "username": "renamed",
                "bio": "プロフィールの説明",
                "avatar_url": "https://example.com/a.png",
            }),
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK, "更新は成功する");

    let body = res.json::<serde_json::Value>().await.expect("patch body");
    assert_eq!(body["username"], "renamed");
    assert_eq!(body["bio"], "プロフィールの説明");
    assert_eq!(body["avatar_url"], "https://example.com/a.png");
    // 編集できない項目がレスポンスで書き換わっていないこと。
    assert_eq!(body["email"], user.email);
    assert_eq!(body["is_admin"], false);

    let after = me_json(&app).await;
    assert_eq!(after["username"], "renamed", "DB に永続化されている");
    assert_eq!(after["bio"], "プロフィールの説明");
    assert_eq!(after["avatar_url"], "https://example.com/a.png");

    app.cleanup_user(user.id).await;
}

/// 省略した項目は変更しない（PATCH なので全項目送信を強制しない）。
#[tokio::test]
async fn omitted_fields_are_left_untouched() {
    let mut app = TestApp::new().await;

    let user = app.insert_user(false, false).await;
    app.reset_session_client();
    app.login_session_no_content(&user.email, &user.password)
        .await;

    app.patch_json_with_session(
        "/v1/auth/me",
        serde_json::json!({ "bio": "最初の説明", "avatar_url": "https://example.com/a.png" }),
    )
    .await;

    let res = app
        .patch_json_with_session("/v1/auth/me", serde_json::json!({ "username": "renamed" }))
        .await;
    assert_eq!(res.status(), StatusCode::OK);

    let after = me_json(&app).await;
    assert_eq!(after["username"], "renamed");
    assert_eq!(after["bio"], "最初の説明", "送っていない bio は消えない");
    assert_eq!(
        after["avatar_url"], "https://example.com/a.png",
        "送っていない avatar_url は消えない"
    );

    app.cleanup_user(user.id).await;
}

/// avatar_url は空文字が URL 検証を通らないため、clear_avatar_url で明示的に外す。
/// bio は空文字がそのまま有効な値。
#[tokio::test]
async fn clears_avatar_url_and_empties_bio() {
    let mut app = TestApp::new().await;

    let user = app.insert_user(false, false).await;
    app.reset_session_client();
    app.login_session_no_content(&user.email, &user.password)
        .await;

    app.patch_json_with_session(
        "/v1/auth/me",
        serde_json::json!({ "bio": "消す前", "avatar_url": "https://example.com/a.png" }),
    )
    .await;

    let res = app
        .patch_json_with_session(
            "/v1/auth/me",
            serde_json::json!({ "bio": "", "clear_avatar_url": true }),
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);

    let after = me_json(&app).await;
    assert_eq!(after["bio"], "", "bio は空文字になる");
    assert!(after["avatar_url"].is_null(), "avatar_url が null になる");

    app.cleanup_user(user.id).await;
}

/// 変更点のない PATCH でも 200 を返し、値を壊さない。
#[tokio::test]
async fn empty_patch_is_a_no_op() {
    let mut app = TestApp::new().await;

    let user = app.insert_user(false, false).await;
    app.reset_session_client();
    app.login_session_no_content(&user.email, &user.password)
        .await;

    let before = me_json(&app).await;

    let res = app
        .patch_json_with_session("/v1/auth/me", serde_json::json!({}))
        .await;
    assert_eq!(res.status(), StatusCode::OK, "空の PATCH でも 500 にしない");

    let after = me_json(&app).await;
    assert_eq!(before["username"], after["username"]);
    assert_eq!(before["bio"], after["bio"]);

    app.cleanup_user(user.id).await;
}

/// `<img src>` に流し込む avatar_url は http/https 以外を受け付けない。
#[tokio::test]
async fn rejects_non_http_avatar_url() {
    let mut app = TestApp::new().await;

    let user = app.insert_user(false, false).await;
    app.reset_session_client();
    app.login_session_no_content(&user.email, &user.password)
        .await;

    for bad in [
        "javascript:alert(1)",
        "data:text/html;base64,PHN2Zz4=",
        "/relative/path.png",
    ] {
        let res = app
            .patch_json_with_session("/v1/auth/me", serde_json::json!({ "avatar_url": bad }))
            .await;
        assert_eq!(
            res.status(),
            StatusCode::BAD_REQUEST,
            "{bad} は受け付けない"
        );
    }

    // 対照: http/https は通る（過剰な拒否になっていないこと）。
    let ok = app
        .patch_json_with_session(
            "/v1/auth/me",
            serde_json::json!({ "avatar_url": "http://example.com/a.png" }),
        )
        .await;
    assert_eq!(ok.status(), StatusCode::OK, "http は通る");

    let after = me_json(&app).await;
    assert_eq!(after["avatar_url"], "http://example.com/a.png");

    app.cleanup_user(user.id).await;
}

/// username の長さ制限。
#[tokio::test]
async fn rejects_out_of_range_username() {
    let mut app = TestApp::new().await;

    let user = app.insert_user(false, false).await;
    app.reset_session_client();
    app.login_session_no_content(&user.email, &user.password)
        .await;

    let too_short = app
        .patch_json_with_session("/v1/auth/me", serde_json::json!({ "username": "ab" }))
        .await;
    assert_eq!(
        too_short.status(),
        StatusCode::BAD_REQUEST,
        "3 文字未満は拒否"
    );

    let too_long = app
        .patch_json_with_session(
            "/v1/auth/me",
            serde_json::json!({ "username": "a".repeat(256) }),
        )
        .await;
    assert_eq!(
        too_long.status(),
        StatusCode::BAD_REQUEST,
        "255 文字超は拒否"
    );

    let ok = app
        .patch_json_with_session("/v1/auth/me", serde_json::json!({ "username": "abc" }))
        .await;
    assert_eq!(ok.status(), StatusCode::OK, "境界の 3 文字は通る");

    app.cleanup_user(user.id).await;
}

/// 未ログインでは更新できない。
#[tokio::test]
async fn rejects_unauthenticated_request() {
    let mut app = TestApp::new().await;
    app.reset_session_client();

    let res = app
        .patch_json_with_session("/v1/auth/me", serde_json::json!({ "username": "renamed" }))
        .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

/// PAT（Bearer）ではプロフィールを書き換えられない。
/// GET /v1/auth/me と同じ CurrentUser の拒否をこの更新経路でも通す。
#[tokio::test]
async fn rejects_bearer_token_request() {
    let mut app = TestApp::new().await;

    let user = app.insert_user(false, false).await;
    app.reset_session_client();
    app.login_session_no_content(&user.email, &user.password)
        .await;

    let rejected = app
        .client()
        .patch(format!("{}/v1/auth/me", app.base_url()))
        .header(reqwest::header::AUTHORIZATION, "Bearer not-a-real-token")
        .json(&serde_json::json!({ "username": "renamed" }))
        .send()
        .await
        .expect("patch me with bearer");
    assert_eq!(
        rejected.status(),
        StatusCode::UNAUTHORIZED,
        "Bearer 付きは拒否される"
    );

    let after = me_json(&app).await;
    assert_ne!(after["username"], "renamed", "書き換わっていない");

    app.cleanup_user(user.id).await;
}
