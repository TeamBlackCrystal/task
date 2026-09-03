mod common;

use axum::http::StatusCode;
use common::TestApp;

// 担当者候補（`GET /projects/{id}/assignable-users`）の統合テスト。
//
// メンバー一覧（`/members`）はメンバー管理の口でプロジェクト管理者しか読めないが、
// 担当者の割り当ては WriteTask があればできる。候補をメンバー一覧から取ると、
// タスクを編集できる人が候補だけ 403 になって担当者を触れなくなる。
// 返す集合も違う: メンバーを 1 人も指定していない共有プロジェクトはテナント全体へ
// 開放されるため、`project_members` の行だけを返すと候補が空になる。

async fn json_body(res: reqwest::Response) -> serde_json::Value {
    res.json::<serde_json::Value>().await.expect("json body")
}

fn user_ids(body: &serde_json::Value) -> Vec<String> {
    body.as_array()
        .expect("user list")
        .iter()
        .map(|u| {
            assert!(u["username"].is_string(), "表示に使う username を含む");
            u["id"].as_str().expect("id").to_string()
        })
        .collect()
}

/// メンバー指定のないプロジェクトは、テナント全体が担当者候補になる。
/// 管理者でないテナントメンバーからも読める（メンバー一覧は 403 のまま）。
#[tokio::test]
async fn assignable_users_covers_tenant_when_project_has_no_members() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let editor = app.insert_user(false, false).await;
    let outsider = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;

    let tenant_members_path = format!("/v1/tenants/{}/members", tp.tenant_id);
    let assignable_path = format!(
        "/v1/tenants/{}/projects/{}/assignable-users",
        tp.tenant_id, tp.project_id
    );
    let members_path = format!(
        "/v1/tenants/{}/projects/{}/members",
        tp.tenant_id, tp.project_id
    );

    app.reset_session_client();
    app.login_session(&owner.email, &owner.password).await;
    let added = app
        .post_json_with_session(
            &tenant_members_path,
            serde_json::json!({ "user_id": editor.id, "role": "Member" }),
        )
        .await;
    assert_eq!(added.status(), StatusCode::CREATED);

    // オーナーから見た候補: オーナー本人とテナントメンバー
    let list = app.get_with_session(&assignable_path).await;
    assert_eq!(list.status(), StatusCode::OK);
    let ids = user_ids(&json_body(list).await);
    assert!(
        ids.contains(&owner.id.to_string()),
        "オーナーは常に候補に入る"
    );
    assert!(
        ids.contains(&editor.id.to_string()),
        "メンバー指定が無いプロジェクトはテナント全体へ開放される"
    );
    assert!(
        !ids.contains(&outsider.id.to_string()),
        "テナント外の利用者は候補に入らない"
    );

    // 管理者でないテナントメンバーからも候補は読める
    app.reset_session_client();
    app.login_session(&editor.email, &editor.password).await;

    let as_editor = app.get_with_session(&assignable_path).await;
    assert_eq!(
        as_editor.status(),
        StatusCode::OK,
        "タスクを編集できる人は候補を読める"
    );
    let ids = user_ids(&json_body(as_editor).await);
    assert!(ids.contains(&owner.id.to_string()));
    assert!(ids.contains(&editor.id.to_string()));

    // 対照: メンバー管理の口は管理者専用のまま
    let members = app.get_with_session(&members_path).await;
    assert_eq!(
        members.status(),
        StatusCode::FORBIDDEN,
        "メンバー一覧の認可は緩めない"
    );
}

/// メンバーを指定したプロジェクトでは、指定された人（＋オーナー）だけが候補になる。
#[tokio::test]
async fn assignable_users_is_limited_to_named_members() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let named = app.insert_user(false, false).await;
    let other = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;

    let tenant_members_path = format!("/v1/tenants/{}/members", tp.tenant_id);
    let members_path = format!(
        "/v1/tenants/{}/projects/{}/members",
        tp.tenant_id, tp.project_id
    );
    let assignable_path = format!(
        "/v1/tenants/{}/projects/{}/assignable-users",
        tp.tenant_id, tp.project_id
    );

    app.reset_session_client();
    app.login_session(&owner.email, &owner.password).await;
    for user in [&named, &other] {
        let added = app
            .post_json_with_session(
                &tenant_members_path,
                serde_json::json!({ "user_id": user.id, "role": "Member" }),
            )
            .await;
        assert_eq!(added.status(), StatusCode::CREATED);
    }
    let added = app
        .post_json_with_session(
            &members_path,
            serde_json::json!({ "user_id": named.id, "role": "Member" }),
        )
        .await;
    assert_eq!(added.status(), StatusCode::CREATED);

    let list = app.get_with_session(&assignable_path).await;
    assert_eq!(list.status(), StatusCode::OK);
    let ids = user_ids(&json_body(list).await);
    assert!(
        ids.contains(&owner.id.to_string()),
        "オーナーは常に候補に入る"
    );
    assert!(ids.contains(&named.id.to_string()));
    assert!(
        !ids.contains(&other.id.to_string()),
        "メンバー指定があるプロジェクトでは指定外を候補にしない"
    );
}

/// テナントに入れない利用者は候補を読めない。
#[tokio::test]
async fn assignable_users_rejects_outsiders() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let outsider = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;

    let assignable_path = format!(
        "/v1/tenants/{}/projects/{}/assignable-users",
        tp.tenant_id, tp.project_id
    );

    app.reset_session_client();
    app.login_session(&outsider.email, &outsider.password).await;

    let res = app.get_with_session(&assignable_path).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

/// 候補として返した利用者は、実際に担当者として割り当てられる
/// （候補一覧と割り当て API の規則がずれていないこと）。
#[tokio::test]
async fn assignable_users_can_actually_be_assigned() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let editor = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;

    let tenant_members_path = format!("/v1/tenants/{}/members", tp.tenant_id);
    let tasks_path = format!(
        "/v1/tenants/{}/projects/{}/tasks",
        tp.tenant_id, tp.project_id
    );
    let assignable_path = format!(
        "/v1/tenants/{}/projects/{}/assignable-users",
        tp.tenant_id, tp.project_id
    );

    app.reset_session_client();
    app.login_session(&owner.email, &owner.password).await;
    let added = app
        .post_json_with_session(
            &tenant_members_path,
            serde_json::json!({ "user_id": editor.id, "role": "Member" }),
        )
        .await;
    assert_eq!(added.status(), StatusCode::CREATED);

    let status_path = format!(
        "/v1/tenants/{}/projects/{}/statuses",
        tp.tenant_id, tp.project_id
    );
    let status = app
        .post_json_with_session(
            &status_path,
            serde_json::json!({
                "name": "Backlog",
                "color": "#336699",
                "position": 0,
                "is_default": true
            }),
        )
        .await;
    assert_eq!(status.status(), StatusCode::CREATED);
    let status_id = json_body(status).await["id"]
        .as_str()
        .expect("status id")
        .to_string();

    let created = app
        .post_json_with_session(
            &tasks_path,
            serde_json::json!({ "title": "担当者を付ける", "status_id": status_id }),
        )
        .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let task_id = json_body(created).await["id"]
        .as_str()
        .expect("task id")
        .to_string();

    // 管理者でないメンバーとして、候補を読んでそのまま割り当てる
    app.reset_session_client();
    app.login_session(&editor.email, &editor.password).await;

    let list = app.get_with_session(&assignable_path).await;
    assert_eq!(list.status(), StatusCode::OK);
    let body = json_body(list).await;
    let candidate_id = body
        .as_array()
        .expect("user list")
        .iter()
        .find(|u| u["id"] == serde_json::json!(editor.id.to_string()))
        .expect("候補に自分が入る")["id"]
        .as_str()
        .expect("id")
        .to_string();

    let assigned = app
        .post_json_with_session(
            &format!("{tasks_path}/{task_id}/assignees"),
            serde_json::json!({ "user_id": candidate_id, "role": "assignee" }),
        )
        .await;
    assert_eq!(
        assigned.status(),
        StatusCode::CREATED,
        "候補として返した利用者は実際に割り当てられる"
    );
}
