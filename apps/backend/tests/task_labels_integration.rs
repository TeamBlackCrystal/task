mod common;

use axum::http::StatusCode;
use common::TestApp;
use serde_json::Value;

/// タスクのラベル付与（作成時 label_ids・更新時 label_ids 置き換え・レスポンス埋め込み）の統合テスト。
#[tokio::test]
async fn task_labels_suite() {
    let mut app = TestApp::new().await;
    let user = app.insert_user(true, false).await;
    app.login_session_no_content(&user.email, &user.password)
        .await;
    let tp = app.insert_tenant_project(user.id).await;

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
    let status_body: Value = status.json().await.expect("status json");
    let status_id = status_body["id"].as_str().expect("status id");

    let labels_path = format!(
        "/v1/tenants/{}/projects/{}/labels",
        tp.tenant_id, tp.project_id
    );
    let mut label_ids = Vec::new();
    for (name, color) in [("bug", "#e11d48"), ("feature", "#3b82f6")] {
        let resp = app
            .post_json_with_session(
                &labels_path,
                serde_json::json!({ "name": name, "color": color }),
            )
            .await;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body: Value = resp.json().await.expect("label json");
        label_ids.push(body["id"].as_str().expect("label id").to_string());
    }

    // 作成時にラベルを付与でき、レスポンスに labels が埋め込まれる
    let tasks_path = format!(
        "/v1/tenants/{}/projects/{}/tasks",
        tp.tenant_id, tp.project_id
    );
    let created = app
        .post_json_with_session(
            &tasks_path,
            serde_json::json!({
                "title": "ラベル付きタスク",
                "status_id": status_id,
                "label_ids": [label_ids[0]]
            }),
        )
        .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let created_body: Value = created.json().await.expect("created json");
    let task_id = created_body["id"].as_str().expect("task id").to_string();
    let created_labels = created_body["labels"].as_array().expect("labels array");
    assert_eq!(created_labels.len(), 1);
    assert_eq!(created_labels[0]["name"], "bug");

    // label_ids だけのボディで置き換え更新できる（他フィールド変更なしでも 200）
    let task_path = format!("{tasks_path}/{task_id}");
    let replaced = app
        .put_json_with_session(
            &task_path,
            serde_json::json!({ "label_ids": [label_ids[1]] }),
        )
        .await;
    assert_eq!(replaced.status(), StatusCode::OK);
    let replaced_body: Value = replaced.json().await.expect("replaced json");
    let replaced_labels = replaced_body["labels"].as_array().expect("labels array");
    assert_eq!(replaced_labels.len(), 1);
    assert_eq!(replaced_labels[0]["name"], "feature");

    // GET 詳細にも labels が載る
    let detail = app.get_with_session(&task_path).await;
    assert_eq!(detail.status(), StatusCode::OK);
    let detail_body: Value = detail.json().await.expect("detail json");
    assert_eq!(detail_body["labels"].as_array().expect("labels").len(), 1);

    // 一覧にも labels が載る
    let list = app.get_with_session(&tasks_path).await;
    assert_eq!(list.status(), StatusCode::OK);
    let list_body: Value = list.json().await.expect("list json");
    let listed_task = list_body["tasks"]
        .as_array()
        .expect("tasks array")
        .iter()
        .find(|t| t["id"] == task_id.as_str())
        .expect("task in list");
    assert_eq!(listed_task["labels"][0]["name"], "feature");

    // 別プロジェクトのラベル ID は 400（付け替えも起きない）
    let other = app.insert_tenant_project(user.id).await;
    let other_labels_path = format!(
        "/v1/tenants/{}/projects/{}/labels",
        other.tenant_id, other.project_id
    );
    let foreign = app
        .post_json_with_session(
            &other_labels_path,
            serde_json::json!({ "name": "foreign", "color": "#000000" }),
        )
        .await;
    assert_eq!(foreign.status(), StatusCode::CREATED);
    let foreign_body: Value = foreign.json().await.expect("foreign json");
    let foreign_id = foreign_body["id"].as_str().expect("foreign id");

    let rejected = app
        .put_json_with_session(&task_path, serde_json::json!({ "label_ids": [foreign_id] }))
        .await;
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
    let after_reject = app.get_with_session(&task_path).await;
    let after_reject_body: Value = after_reject.json().await.expect("after reject json");
    assert_eq!(after_reject_body["labels"][0]["name"], "feature");

    // 空配列で全解除
    let cleared = app
        .put_json_with_session(&task_path, serde_json::json!({ "label_ids": [] }))
        .await;
    assert_eq!(cleared.status(), StatusCode::OK);
    let cleared_body: Value = cleared.json().await.expect("cleared json");
    assert_eq!(cleared_body["labels"].as_array().expect("labels").len(), 0);

    // label_ids を送らなければ既存ラベルは維持される
    let relabel = app
        .put_json_with_session(
            &task_path,
            serde_json::json!({ "label_ids": [label_ids[0], label_ids[1]] }),
        )
        .await;
    assert_eq!(relabel.status(), StatusCode::OK);
    let untouched = app
        .put_json_with_session(&task_path, serde_json::json!({ "title": "改題" }))
        .await;
    assert_eq!(untouched.status(), StatusCode::OK);
    let untouched_body: Value = untouched.json().await.expect("untouched json");
    assert_eq!(
        untouched_body["labels"].as_array().expect("labels").len(),
        2
    );
}
