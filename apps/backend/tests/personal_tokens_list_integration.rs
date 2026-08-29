mod common;

use axum::http::StatusCode;
use common::{TestApp, insert_personal_token_for_test, insert_tenant};

/// 作成 API でトークンを 1 件発行し、その ID を返す。
async fn create_token(app: &TestApp, tenant_id: uuid::Uuid, name: &str) -> uuid::Uuid {
    let res = app
        .post_json_with_session(
            "/v1/personal_tokens",
            serde_json::json!({
                "name": name,
                "tenant_id": tenant_id,
                "scopes": ["read:task"],
            }),
        )
        .await;
    assert_eq!(res.status(), StatusCode::CREATED, "トークン発行は成功する");
    let body = res.json::<serde_json::Value>().await.expect("create body");
    body["id"].as_str().expect("id").parse().expect("uuid")
}

async fn list_tokens(app: &TestApp) -> Vec<serde_json::Value> {
    let res = app.get_with_session("/v1/personal_tokens").await;
    assert_eq!(res.status(), StatusCode::OK);
    res.json::<Vec<serde_json::Value>>()
        .await
        .expect("list body")
}

/// 自分の有効なトークンだけが名前順で返り、他ユーザーの分は混ざらない。
/// 平文トークン・ハッシュは一覧に含まれない。
#[tokio::test]
async fn lists_only_own_active_tokens() {
    let mut app = TestApp::new().await;

    let user = app.insert_user_default().await;
    let other = app.insert_user_default().await;
    let tenant_id = insert_tenant(&app.state.db, user.id).await;
    let other_tenant_id = insert_tenant(&app.state.db, other.id).await;

    // 他ユーザーのトークン（見えてはいけない）。
    let secret = app.state.settings.personal_token_secret.clone();
    insert_personal_token_for_test(&app.state.db, other.id, other_tenant_id, &secret).await;

    app.reset_session_client();
    app.login_session_no_content(&user.email, &user.password)
        .await;

    // 名前順を確認するため、あえて逆順で作る。
    create_token(&app, tenant_id, "b-second").await;
    create_token(&app, tenant_id, "a-first").await;

    let tokens = list_tokens(&app).await;
    assert_eq!(tokens.len(), 2, "自分のトークンだけが返る");
    assert_eq!(tokens[0]["name"], "a-first", "名前の昇順で返る");
    assert_eq!(tokens[1]["name"], "b-second");

    for token in &tokens {
        assert_eq!(token["user_id"], user.id.to_string());
        assert_eq!(token["tenant_id"], tenant_id.to_string());
        assert_eq!(token["revoked"], false);
        assert_eq!(token["scopes"], serde_json::json!(["read:task"]));
        assert_eq!(
            token["token_last_four"].as_str().expect("last four").len(),
            4
        );
        // 平文トークンとハッシュは作成応答以外で返さない。
        assert!(token.get("token").is_none(), "平文トークンを含まない");
        assert!(token.get("token_hash").is_none(), "ハッシュを含まない");
    }

    app.cleanup_user(user.id).await;
    app.cleanup_user(other.id).await;
}

/// 取り消したトークンは一覧から消える（取り消し前は載っている対照付き）。
#[tokio::test]
async fn revoked_tokens_disappear_from_list() {
    let mut app = TestApp::new().await;

    let user = app.insert_user_default().await;
    let tenant_id = insert_tenant(&app.state.db, user.id).await;

    app.reset_session_client();
    app.login_session_no_content(&user.email, &user.password)
        .await;

    let keep_id = create_token(&app, tenant_id, "keep").await;
    let revoke_id = create_token(&app, tenant_id, "revoke-me").await;

    let before = list_tokens(&app).await;
    assert_eq!(before.len(), 2, "取り消し前は両方載っている");

    let res = app
        .delete_with_session(&format!("/v1/personal_tokens/{revoke_id}"))
        .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let after = list_tokens(&app).await;
    assert_eq!(after.len(), 1, "取り消した分だけ消える");
    assert_eq!(after[0]["id"], keep_id.to_string());

    app.cleanup_user(user.id).await;
}

/// トークンを 1 件も持たないユーザーには空配列が返る（404 等にしない）。
#[tokio::test]
async fn empty_list_for_user_without_tokens() {
    let mut app = TestApp::new().await;

    let user = app.insert_user_default().await;
    app.reset_session_client();
    app.login_session_no_content(&user.email, &user.password)
        .await;

    let tokens = list_tokens(&app).await;
    assert!(tokens.is_empty());

    app.cleanup_user(user.id).await;
}

/// 未ログインは拒否される。
#[tokio::test]
async fn rejects_unauthenticated() {
    let mut app = TestApp::new().await;
    app.reset_session_client();

    let res = app.get_with_session("/v1/personal_tokens").await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

/// PAT（Bearer）ではトークン管理 API を使えない（セッション専用）。
#[tokio::test]
async fn rejects_bearer_token_auth() {
    let app = TestApp::new().await;

    let user = app.insert_user_default().await;
    let tenant_id = insert_tenant(&app.state.db, user.id).await;
    let secret = app.state.settings.personal_token_secret.clone();
    let token = insert_personal_token_for_test(&app.state.db, user.id, tenant_id, &secret).await;

    let res = app.get_with_bearer("/v1/personal_tokens", &token).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    app.cleanup_user(user.id).await;
}
