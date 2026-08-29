mod common;

use axum::http::StatusCode;
use common::TestApp;

// プロジェクトメンバー管理（#317）の統合テスト。
//
// メンバー管理 UI が名前・アバターを表示できるよう、メンバー系レスポンスに
// `user`（UserSummary）を同梱したことの回帰テストを中心に置く。
// `user` フィールドの検証は変更前の main では fail する（フィールド自体が無い）。

async fn json_body(res: reqwest::Response) -> serde_json::Value {
    res.json::<serde_json::Value>().await.expect("json body")
}

/// 一覧・追加・変更のレスポンスに表示用のユーザー情報が同梱される。
#[tokio::test]
async fn project_member_responses_embed_user_summary() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let member = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;

    let tenant_members_path = format!("/v1/tenants/{}/members", tp.tenant_id);
    let project_members_path = format!(
        "/v1/tenants/{}/projects/{}/members",
        tp.tenant_id, tp.project_id
    );

    app.reset_session_client();
    app.login_session(&owner.email, &owner.password).await;

    // プロジェクトメンバーにするには先にテナントメンバーであることが要る（#568）
    let added_tenant = app
        .post_json_with_session(
            &tenant_members_path,
            serde_json::json!({ "user_id": member.id, "role": "Member" }),
        )
        .await;
    assert_eq!(added_tenant.status(), StatusCode::CREATED);
    let added_tenant_body = json_body(added_tenant).await;
    assert_eq!(
        added_tenant_body["user"]["id"],
        member.id.to_string(),
        "テナントメンバー追加のレスポンスにユーザー情報が同梱される"
    );
    assert!(
        added_tenant_body["user"]["username"].is_string(),
        "username を含む"
    );

    // 追加のレスポンス
    let added = app
        .post_json_with_session(
            &project_members_path,
            serde_json::json!({ "user_id": member.id, "role": "Member" }),
        )
        .await;
    assert_eq!(added.status(), StatusCode::CREATED);
    let added_body = json_body(added).await;
    assert_eq!(added_body["user_id"], member.id.to_string());
    assert_eq!(added_body["user"]["id"], member.id.to_string());
    assert!(added_body["user"]["username"].is_string());
    assert!(
        added_body["user"].get("email").is_none(),
        "UserSummary にメールアドレスは含めない"
    );

    // 一覧のレスポンス
    let list = app.get_with_session(&project_members_path).await;
    assert_eq!(list.status(), StatusCode::OK);
    let list_body = json_body(list).await;
    let rows = list_body.as_array().expect("member list");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["user"]["id"], member.id.to_string());
    assert!(rows[0]["user"]["username"].is_string());

    // 変更のレスポンス
    let updated = app
        .put_json_with_session(
            &format!("{project_members_path}/{}", member.id),
            serde_json::json!({ "role": "Admin" }),
        )
        .await;
    assert_eq!(updated.status(), StatusCode::OK);
    let updated_body = json_body(updated).await;
    assert_eq!(updated_body["role"], "Admin");
    assert_eq!(updated_body["user"]["id"], member.id.to_string());

    // テナントメンバー一覧のレスポンス
    let tenant_list = app.get_with_session(&tenant_members_path).await;
    assert_eq!(tenant_list.status(), StatusCode::OK);
    let tenant_rows = json_body(tenant_list).await;
    let tenant_rows = tenant_rows.as_array().expect("tenant member list");
    assert_eq!(tenant_rows.len(), 1);
    assert_eq!(tenant_rows[0]["user"]["id"], member.id.to_string());
    assert!(tenant_rows[0]["user"]["username"].is_string());

    app.cleanup_user(owner.id).await;
    app.cleanup_user(member.id).await;
}

/// 一覧を見られるのはオーナーとプロジェクト Admin だけ（UI の 403 表示の前提）。
/// Admin へ昇格すると見られるようになる対照付き。
#[tokio::test]
async fn project_member_list_requires_project_admin() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let member = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;

    let tenant_members_path = format!("/v1/tenants/{}/members", tp.tenant_id);
    let project_members_path = format!(
        "/v1/tenants/{}/projects/{}/members",
        tp.tenant_id, tp.project_id
    );

    app.reset_session_client();
    app.login_session(&owner.email, &owner.password).await;
    assert_eq!(
        app.post_json_with_session(
            &tenant_members_path,
            serde_json::json!({ "user_id": member.id, "role": "Member" }),
        )
        .await
        .status(),
        StatusCode::CREATED
    );
    assert_eq!(
        app.post_json_with_session(
            &project_members_path,
            serde_json::json!({ "user_id": member.id, "role": "Member" }),
        )
        .await
        .status(),
        StatusCode::CREATED
    );

    // Member ロールでは一覧を見られない
    app.reset_session_client();
    app.login_session(&member.email, &member.password).await;
    assert_eq!(
        app.get_with_session(&project_members_path).await.status(),
        StatusCode::FORBIDDEN,
        "プロジェクト Admin でないメンバーは一覧を見られない"
    );

    // Admin へ昇格すると見られる（過剰拒否でないことの対照）
    app.reset_session_client();
    app.login_session(&owner.email, &owner.password).await;
    assert_eq!(
        app.put_json_with_session(
            &format!("{project_members_path}/{}", member.id),
            serde_json::json!({ "role": "Admin" }),
        )
        .await
        .status(),
        StatusCode::OK
    );

    app.reset_session_client();
    app.login_session(&member.email, &member.password).await;
    let list = app.get_with_session(&project_members_path).await;
    assert_eq!(list.status(), StatusCode::OK, "Admin なら一覧を見られる");

    app.cleanup_user(owner.id).await;
    app.cleanup_user(member.id).await;
}

/// 同時に相手を降格させても、Admin が 0 人にはならない。
///
/// 「最後の Admin を残す」判定は数えてから書くので、直列化しないと双方が
/// 「まだもう 1 人いる」を読んで両方通る。Admin が 0 人になるとプロジェクト側から
/// 誰も直せなくなり、admin_ids が空になるので 409 のガード自体も効かなくなる。
#[tokio::test]
async fn concurrent_demotions_cannot_drop_the_last_admin() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let alice = app.insert_user(false, false).await;
    let bob = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;

    let tenant_members_path = format!("/v1/tenants/{}/members", tp.tenant_id);
    let project_members_path = format!(
        "/v1/tenants/{}/projects/{}/members",
        tp.tenant_id, tp.project_id
    );

    app.reset_session_client();
    app.login_session(&owner.email, &owner.password).await;
    for user in [&alice, &bob] {
        assert_eq!(
            app.post_json_with_session(
                &tenant_members_path,
                serde_json::json!({ "user_id": user.id, "role": "Member" }),
            )
            .await
            .status(),
            StatusCode::CREATED
        );
        assert_eq!(
            app.post_json_with_session(
                &project_members_path,
                serde_json::json!({ "user_id": user.id, "role": "Admin" }),
            )
            .await
            .status(),
            StatusCode::CREATED
        );
    }

    // 2 人の Admin を同時に Viewer へ落とす
    let alice_path = format!("{project_members_path}/{}", alice.id);
    let bob_path = format!("{project_members_path}/{}", bob.id);
    let viewer = serde_json::json!({ "role": "Viewer" });
    let (first, second) = tokio::join!(
        app.put_json_with_session(&alice_path, viewer.clone()),
        app.put_json_with_session(&bob_path, viewer.clone()),
    );
    let statuses = [first.status(), second.status()];
    assert!(
        statuses.contains(&StatusCode::CONFLICT),
        "片方は最後の管理者として弾かれる: {statuses:?}"
    );

    // Admin は 1 人残っている（0 人になると誰も直せなくなる）
    let members = json_body(app.get_with_session(&project_members_path).await).await;
    let admins = members
        .as_array()
        .expect("members")
        .iter()
        .filter(|m| m["role"] == "Admin")
        .count();
    assert_eq!(admins, 1, "Admin が 0 人にならない: {members}");

    app.cleanup_user(owner.id).await;
    app.cleanup_user(alice.id).await;
    app.cleanup_user(bob.id).await;
}
