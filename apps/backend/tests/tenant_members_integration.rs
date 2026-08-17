mod common;

use axum::http::StatusCode;
use common::TestApp;
use serde_json::Value;
use uuid::Uuid;

/// テナントメンバー（#568）の統合テスト。
///
/// - テナントに入れるのはオーナーとテナントメンバーだけ
/// - プロジェクトにメンバーを 1 人も指定していなければ、テナントメンバー全員が入れる
/// - プロジェクトにメンバーを指定した場合は、その中に居る人だけが入れる
#[tokio::test]
async fn tenant_members_gate_tenant_and_project_access() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let member = app.insert_user(false, false).await;
    let outsider = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;

    let members_path = format!("/v1/tenants/{}/members", tp.tenant_id);
    let tenant_path = format!("/v1/tenants/{}", tp.tenant_id);
    let project_path = format!("/v1/tenants/{}/projects/{}", tp.tenant_id, tp.project_id);

    // --- 追加前: メンバーでない人はテナントを見られない ---
    app.reset_session_client();
    app.login_session(&member.email, &member.password).await;
    assert_eq!(
        app.get_with_session(&tenant_path).await.status(),
        StatusCode::FORBIDDEN,
        "テナントメンバーでなければテナントを取得できない"
    );
    assert!(
        !tenant_ids(app.get_with_session("/v1/tenants").await)
            .await
            .contains(&tp.tenant_id),
        "テナントメンバーでなければ一覧にも出ない"
    );

    // --- オーナーがテナントメンバーに追加する ---
    app.reset_session_client();
    app.login_session(&owner.email, &owner.password).await;
    let added = app
        .post_json_with_session(
            &members_path,
            serde_json::json!({ "user_id": member.id, "role": "Member" }),
        )
        .await;
    assert_eq!(added.status(), StatusCode::CREATED);

    // 同じ人を二重に追加すると 409
    let duplicated = app
        .post_json_with_session(
            &members_path,
            serde_json::json!({ "user_id": member.id, "role": "Member" }),
        )
        .await;
    assert_eq!(duplicated.status(), StatusCode::CONFLICT);

    // --- 追加後: テナントもプロジェクトも見える（project_members が空なので開放） ---
    app.reset_session_client();
    app.login_session(&member.email, &member.password).await;
    assert_eq!(
        app.get_with_session(&tenant_path).await.status(),
        StatusCode::OK,
        "テナントメンバーはテナントを取得できる"
    );
    assert!(
        tenant_ids(app.get_with_session("/v1/tenants").await)
            .await
            .contains(&tp.tenant_id),
        "テナントメンバーは一覧に出る"
    );
    assert_eq!(
        app.get_with_session(&project_path).await.status(),
        StatusCode::OK,
        "メンバー未指定のプロジェクトはテナントメンバー全員に開放される"
    );

    // 無関係なユーザーは依然として入れない（過剰に開放していないこと）
    app.reset_session_client();
    app.login_session(&outsider.email, &outsider.password).await;
    assert_eq!(
        app.get_with_session(&tenant_path).await.status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.get_with_session(&project_path).await.status(),
        StatusCode::FORBIDDEN
    );

    // --- プロジェクトにメンバーを指定すると、その人以外は弾かれる ---
    app.reset_session_client();
    app.login_session(&owner.email, &owner.password).await;
    let project_members_path = format!(
        "/v1/tenants/{}/projects/{}/members",
        tp.tenant_id, tp.project_id
    );
    // テナントに居ない人はプロジェクトメンバーにできない
    // （「プロジェクトには居るがテナントには入れない」不整合を作らせない）
    let rejected = app
        .post_json_with_session(
            &project_members_path,
            serde_json::json!({ "user_id": outsider.id, "role": "Member" }),
        )
        .await;
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);

    // テナントメンバーを 1 人プロジェクトに指定する
    let assignee = app.insert_user(false, false).await;
    let joined = app
        .post_json_with_session(
            &members_path,
            serde_json::json!({ "user_id": assignee.id, "role": "Member" }),
        )
        .await;
    assert_eq!(joined.status(), StatusCode::CREATED);
    let assigned = app
        .post_json_with_session(
            &project_members_path,
            serde_json::json!({ "user_id": assignee.id, "role": "Member" }),
        )
        .await;
    assert_eq!(assigned.status(), StatusCode::CREATED);

    app.reset_session_client();
    app.login_session(&member.email, &member.password).await;
    assert_eq!(
        app.get_with_session(&tenant_path).await.status(),
        StatusCode::OK,
        "テナント自体には引き続き入れる"
    );
    assert_eq!(
        app.get_with_session(&project_path).await.status(),
        StatusCode::FORBIDDEN,
        "メンバーを指定したプロジェクトは、指定された人以外は入れない"
    );

    app.cleanup_user(assignee.id).await;
    app.cleanup_user(member.id).await;
    app.cleanup_user(outsider.id).await;
    app.cleanup_user(owner.id).await;
}

/// メンバーの追加・変更・削除はオーナーとテナント Admin だけに許す。
#[tokio::test]
async fn only_owner_and_tenant_admin_can_manage_members() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let admin = app.insert_user(false, false).await;
    let plain = app.insert_user(false, false).await;
    let target = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;
    let members_path = format!("/v1/tenants/{}/members", tp.tenant_id);

    app.reset_session_client();
    app.login_session(&owner.email, &owner.password).await;
    for (user, role) in [(&admin, "Admin"), (&plain, "Member")] {
        let res = app
            .post_json_with_session(
                &members_path,
                serde_json::json!({ "user_id": user.id, "role": role }),
            )
            .await;
        assert_eq!(res.status(), StatusCode::CREATED);
    }

    // Member ロールでは追加できない
    app.reset_session_client();
    app.login_session(&plain.email, &plain.password).await;
    let rejected = app
        .post_json_with_session(
            &members_path,
            serde_json::json!({ "user_id": target.id, "role": "Member" }),
        )
        .await;
    assert_eq!(rejected.status(), StatusCode::FORBIDDEN);
    // 一覧の閲覧は許す（管理操作ではない）
    assert_eq!(
        app.get_with_session(&members_path).await.status(),
        StatusCode::OK
    );

    // Admin ロールなら追加・変更・削除できる
    app.reset_session_client();
    app.login_session(&admin.email, &admin.password).await;
    let accepted = app
        .post_json_with_session(
            &members_path,
            serde_json::json!({ "user_id": target.id, "role": "Viewer" }),
        )
        .await;
    assert_eq!(accepted.status(), StatusCode::CREATED);

    let target_path = format!("{members_path}/{}", target.id);
    let updated = app
        .put_json_with_session(&target_path, serde_json::json!({ "role": "Member" }))
        .await;
    assert_eq!(updated.status(), StatusCode::OK);
    let updated_body: Value = updated.json().await.expect("updated json");
    assert_eq!(updated_body["role"], "Member");

    assert_eq!(
        app.delete_with_session(&target_path).await.status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.delete_with_session(&target_path).await.status(),
        StatusCode::NOT_FOUND,
        "削除済みのメンバーは 404"
    );

    app.cleanup_user(admin.id).await;
    app.cleanup_user(plain.id).await;
    app.cleanup_user(target.id).await;
    app.cleanup_user(owner.id).await;
}

async fn tenant_ids(res: reqwest::Response) -> Vec<Uuid> {
    let body: Value = res.json().await.expect("tenant list json");
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
