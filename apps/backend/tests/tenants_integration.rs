mod common;

use axum::http::StatusCode;
use common::TestApp;
use uuid::Uuid;

/// `POST /v1/tenants` が `display_id` 重複で 409 Conflict を返すこと。
///
/// この回帰テストは #336 で OpenAPI に 409 を宣言する前提となる
/// 実行時契約を固定する。`CrudErrors` に 409 が無くても実行時は 409 が
/// 返っていたが、 spec に明示されない状態ではフロントが生ステータス比較に
/// 頼るしかなく、契約が型システムで追跡できなかった。
#[tokio::test]
async fn create_tenant_duplicate_display_id_returns_409() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    app.reset_session_client();
    app.login_session(&owner.email, &owner.password).await;

    let display_id = format!("dup-{}", &Uuid::new_v4().to_string()[..8]);
    let body = serde_json::json!({
        "display_id": display_id,
        "name": "first tenant",
        "description": "",
        "icon_url": "",
    });

    // 1 件目は 201 で作成される
    let first = app
        .post_json_with_session("/v1/tenants", body.clone())
        .await;
    assert_eq!(
        first.status(),
        StatusCode::CREATED,
        "first create should succeed"
    );

    // 2 件目は display_id 重複で 409
    let second = app.post_json_with_session("/v1/tenants", body).await;
    assert_eq!(
        second.status(),
        StatusCode::CONFLICT,
        "duplicate display_id must return 409 Conflict"
    );

    app.cleanup_user(owner.id).await;
}

/// `GET /v1/tenants` が、プロジェクトメンバーとして参加しているテナントも返すこと。
///
/// 認可側（`session_has_tenant_access`）は project_members 経由の参加を許可していたが、
/// 一覧だけが `owner_id` で絞っていたため、参加者はテナント配下 API を叩けるのに
/// 一覧に出ず、画面上テナントが存在しないように見えていた。
#[tokio::test]
async fn list_tenants_includes_projects_the_user_joined() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let member = app.insert_user(false, false).await;
    let outsider = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;

    // オーナーとしてプロジェクトメンバーに追加する
    app.reset_session_client();
    app.login_session(&owner.email, &owner.password).await;
    let added = app
        .post_json_with_session(
            &format!(
                "/v1/tenants/{}/projects/{}/members",
                tp.tenant_id, tp.project_id
            ),
            serde_json::json!({ "user_id": member.id, "role": "Member" }),
        )
        .await;
    assert!(
        added.status().is_success(),
        "adding a project member should succeed, got {}",
        added.status()
    );

    // オーナー自身は従来どおり自分のテナントが見える
    let owner_list = app.get_with_session("/v1/tenants").await;
    assert_eq!(owner_list.status(), StatusCode::OK);
    assert!(
        tenant_ids(owner_list).await.contains(&tp.tenant_id),
        "owner must still see their own tenant"
    );

    // 参加者にも見える（本来の修正対象）
    app.reset_session_client();
    app.login_session(&member.email, &member.password).await;
    let member_list = app.get_with_session("/v1/tenants").await;
    assert_eq!(member_list.status(), StatusCode::OK);
    assert!(
        tenant_ids(member_list).await.contains(&tp.tenant_id),
        "a project member must see the tenant that owns the project"
    );

    // 無関係なユーザーには見えない（過剰に広げていないこと）
    app.reset_session_client();
    app.login_session(&outsider.email, &outsider.password).await;
    let outsider_list = app.get_with_session("/v1/tenants").await;
    assert_eq!(outsider_list.status(), StatusCode::OK);
    assert!(
        !tenant_ids(outsider_list).await.contains(&tp.tenant_id),
        "an unrelated user must not see the tenant"
    );

    app.cleanup_user(member.id).await;
    app.cleanup_user(outsider.id).await;
    app.cleanup_user(owner.id).await;
}

async fn tenant_ids(res: reqwest::Response) -> Vec<Uuid> {
    let body: serde_json::Value = res.json().await.expect("tenant list json");
    body.as_array()
        .expect("tenant list must be an array")
        .iter()
        .map(|t| {
            t["id"]
                .as_str()
                .and_then(|s| Uuid::parse_str(s).ok())
                .expect("tenant id")
        })
        .collect()
}
