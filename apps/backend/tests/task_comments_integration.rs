mod common;

use axum::http::StatusCode;
use common::TestApp;
use serde_json::Value;

async fn setup_task(app: &mut TestApp) -> (common::TestTenantProject, String, String) {
    let user = app.insert_user(true, false).await;
    app.login_session_no_content(&user.email, &user.password)
        .await;
    let tp = app.insert_tenant_project(user.id).await;

    let status_path = format!(
        "/v1/tenants/{}/projects/{}/statuses",
        tp.tenant_id, tp.project_id
    );
    let status_resp = app
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
    assert_eq!(status_resp.status(), StatusCode::CREATED);
    let status: Value = status_resp.json().await.expect("status json");
    let status_id = status["id"].as_str().expect("status id");

    let tasks_path = format!(
        "/v1/tenants/{}/projects/{}/tasks",
        tp.tenant_id, tp.project_id
    );
    let task_resp = app
        .post_json_with_session(
            &tasks_path,
            serde_json::json!({
                "title": "Collaboration test task",
                "status_id": status_id
            }),
        )
        .await;
    assert_eq!(task_resp.status(), StatusCode::CREATED);
    let task: Value = task_resp.json().await.expect("task json");
    let task_id = task["id"].as_str().expect("task id").to_string();

    (tp, task_id, user.email)
}

#[tokio::test]
async fn task_comments_integration_suite() {
    let mut app = TestApp::new().await;
    let (tp, task_id, _email) = setup_task(&mut app).await;

    let comments_base = format!(
        "/v1/tenants/{}/projects/{}/tasks/{}/comments",
        tp.tenant_id, tp.project_id, task_id
    );
    let activities_path = format!(
        "/v1/tenants/{}/projects/{}/tasks/{}/activities",
        tp.tenant_id, tp.project_id, task_id
    );

    let create_parent = app
        .post_json_with_session(
            &comments_base,
            serde_json::json!({ "body": "設計は完了しました。", "parent_comment_id": null }),
        )
        .await;
    assert_eq!(create_parent.status(), StatusCode::CREATED);
    let parent: Value = create_parent.json().await.expect("parent json");
    let parent_id = parent["id"].as_str().expect("parent id");
    // frontend は updated_at != created_at を「編集済み」と解釈する。
    // now() を 2 回呼ぶ実装に戻るとマイクロ秒差で新規コメントに
    // 「(編集済み)」が付くため、作成直後の完全一致を固定する
    assert_eq!(parent["created_at"], parent["updated_at"]);

    let create_reply = app
        .post_json_with_session(
            &comments_base,
            serde_json::json!({
                "body": "レビュー依頼します。",
                "parent_comment_id": parent_id
            }),
        )
        .await;
    assert_eq!(create_reply.status(), StatusCode::CREATED);
    let reply: Value = create_reply.json().await.expect("reply json");
    let reply_id = reply["id"].as_str().expect("reply id").to_string();

    let list = app.get_with_session(&comments_base).await;
    assert_eq!(list.status(), StatusCode::OK);
    let list_body: Value = list.json().await.expect("list json");
    assert_eq!(list_body["comments"].as_array().expect("comments").len(), 1);
    assert_eq!(
        list_body["comments"][0]["replies"]
            .as_array()
            .expect("replies")
            .len(),
        1
    );

    let activities = app.get_with_session(&activities_path).await;
    assert_eq!(activities.status(), StatusCode::OK);
    let act_body: Value = activities.json().await.expect("activities json");
    let events: Vec<&str> = act_body["activities"]
        .as_array()
        .expect("activities")
        .iter()
        .filter_map(|a| a["event_type"].as_str())
        .collect();
    assert!(events.contains(&"task_created"));
    assert!(events.contains(&"comment_added"));

    let update_path = format!("{comments_base}/{reply_id}");
    let update = app
        .put_json_with_session(
            &update_path,
            serde_json::json!({ "body": "更新した返信です。" }),
        )
        .await;
    assert_eq!(update.status(), StatusCode::OK);

    let delete_parent = app
        .delete_with_session(&format!("{comments_base}/{parent_id}"))
        .await;
    assert_eq!(delete_parent.status(), StatusCode::NO_CONTENT);

    let list_after_delete = app.get_with_session(&comments_base).await;
    let after_body: Value = list_after_delete.json().await.expect("after delete json");
    assert_eq!(after_body["comments"][0]["is_deleted"], true);
    assert!(after_body["comments"][0]["body"].is_null());
    assert_eq!(
        after_body["comments"][0]["replies"]
            .as_array()
            .expect("replies remain")
            .len(),
        1
    );

    let other = app.insert_user(false, false).await;
    app.reset_session_client();
    app.login_session_no_content(&other.email, &other.password)
        .await;
    let forbidden_update = app
        .put_json_with_session(&update_path, serde_json::json!({ "body": "他人の編集" }))
        .await;
    assert_eq!(forbidden_update.status(), StatusCode::FORBIDDEN);
}
