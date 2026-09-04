mod common;

use axum::http::StatusCode;
use common::{TestApp, TestTenantProject, TestUser};
use uuid::Uuid;

fn tasks_base(tp: &TestTenantProject) -> String {
    format!(
        "/v1/tenants/{}/projects/{}/tasks",
        tp.tenant_id, tp.project_id
    )
}

async fn setup_project(app: &mut TestApp) -> (TestUser, TestTenantProject) {
    let user = app.insert_user_default().await;
    app.login_session_no_content(&user.email, &user.password)
        .await;
    let tp = app.insert_tenant_project(user.id).await;
    (user, tp)
}

async fn create_status(app: &TestApp, tp: &TestTenantProject) -> Uuid {
    let path = format!(
        "/v1/tenants/{}/projects/{}/statuses",
        tp.tenant_id, tp.project_id
    );
    let response = app
        .post_json_with_session(
            &path,
            serde_json::json!({
                "name": "Todo",
                "color": "#336699",
                "position": 1,
                "is_default": true,
                "is_done_state": false,
            }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CREATED, "create status");
    let body: serde_json::Value = response.json().await.expect("status json");
    body["id"]
        .as_str()
        .expect("status id")
        .parse()
        .expect("uuid")
}

fn assert_user_summary(value: &serde_json::Value, expected_id: Uuid) {
    assert_eq!(
        value["id"].as_str(),
        Some(expected_id.to_string()).as_deref()
    );
    assert!(!value["username"].as_str().expect("username").is_empty());
    assert!(value.get("email").is_none(), "email must not be embedded");
}

#[tokio::test]
async fn task_responses_include_user_info() {
    let mut app = TestApp::new().await;
    let (user, tp) = setup_project(&mut app).await;
    let status_id = create_status(&app, &tp).await;

    // 担当者あり / なしのタスクを1件ずつ作成
    let with_assignee = app
        .post_json_with_session(
            &tasks_base(&tp),
            serde_json::json!({
                "title": "Assigned task",
                "status_id": status_id,
                "assignees": [{ "user_id": user.id, "role": "reviewer" }],
            }),
        )
        .await;
    assert_eq!(with_assignee.status(), StatusCode::CREATED);
    let created: serde_json::Value = with_assignee.json().await.expect("create json");
    // 作成レスポンス(TaskDetailResponse)にもユーザー情報が埋まる
    assert_user_summary(&created["created_by"], user.id);
    let task_id = created["id"].as_str().expect("task id").to_string();

    let without_assignee = app
        .post_json_with_session(
            &tasks_base(&tp),
            serde_json::json!({
                "title": "Unassigned task",
                "status_id": status_id,
            }),
        )
        .await;
    assert_eq!(without_assignee.status(), StatusCode::CREATED);

    // 一覧
    let response = app.get_with_session(&tasks_base(&tp)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = response.json().await.expect("list json");

    assert_eq!(body["total"].as_u64(), Some(2));
    let tasks = body["tasks"].as_array().expect("tasks array");
    assert_eq!(tasks.len(), 2);

    for task in tasks {
        assert_user_summary(&task["created_by"], user.id);
    }

    let assigned = tasks
        .iter()
        .find(|t| t["title"] == "Assigned task")
        .expect("assigned task in list");
    let assignees = assigned["assignees"].as_array().expect("assignees array");
    assert_eq!(assignees.len(), 1);
    assert_eq!(assignees[0]["role"].as_str(), Some("reviewer"));
    assert_user_summary(&assignees[0]["user"], user.id);

    let unassigned = tasks
        .iter()
        .find(|t| t["title"] == "Unassigned task")
        .expect("unassigned task in list");
    assert_eq!(unassigned["assignees"].as_array().map(Vec::len), Some(0));

    // 詳細も同じスキーマでユーザー情報を返す
    let detail = app
        .get_with_session(&format!("{}/{}", tasks_base(&tp), task_id))
        .await;
    assert_eq!(detail.status(), StatusCode::OK);
    let detail_body: serde_json::Value = detail.json().await.expect("detail json");
    assert_user_summary(&detail_body["created_by"], user.id);
    let detail_assignees = detail_body["assignees"].as_array().expect("assignees");
    assert_eq!(detail_assignees.len(), 1);
    assert_user_summary(&detail_assignees[0]["user"], user.id);

    app.cleanup_user(user.id).await;
}

/// 一覧のページを 1 つ読み、その ID を返す。
async fn list_page_ids(
    app: &TestApp,
    tp: &TestTenantProject,
    sort: &str,
    limit: u64,
    offset: u64,
) -> Vec<String> {
    let path = format!(
        "{}?sort={sort}&limit={limit}&offset={offset}",
        tasks_base(tp)
    );
    let response = app.get_with_session(&path).await;
    assert_eq!(response.status(), StatusCode::OK, "list page");
    let body: serde_json::Value = response.json().await.expect("list json");
    body["tasks"]
        .as_array()
        .expect("tasks array")
        .iter()
        .map(|task| task["id"].as_str().expect("task id").to_string())
        .collect()
}

/// 検索のページを 1 つ読み、その ID を返す。
async fn search_page_ids(
    app: &TestApp,
    tp: &TestTenantProject,
    query: &str,
    limit: u64,
    offset: u64,
) -> Vec<String> {
    let path = format!(
        "{}/search?q={query}&limit={limit}&offset={offset}",
        tasks_base(tp)
    );
    let response = app.get_with_session(&path).await;
    assert_eq!(response.status(), StatusCode::OK, "search page");
    let body: serde_json::Value = response.json().await.expect("search json");
    body["tasks"]
        .as_array()
        .expect("tasks array")
        .iter()
        .map(|hit| hit["id"].as_str().expect("task id").to_string())
        .collect()
}

/// 優先度も検索スコアも同値が並ぶ。並びを 1 列だけで決めると同値行の順序が未定義になり、
/// offset で続きを読んだときにタスクが重複・欠落する。ID を足して一意に決める。
#[tokio::test]
async fn pages_tied_tasks_in_a_single_order_so_none_is_skipped() {
    let mut app = TestApp::new().await;
    let (user, tp) = setup_project(&mut app).await;
    let status_id = create_status(&app, &tp).await;

    // 優先度・期限・検索スコアがどれも同値になる 7 件。
    // ページサイズ 3 の境界（3・6 件目）を越える件数にして、境界の欠落を隠さない
    let mut created = Vec::new();
    for index in 0..7 {
        let response = app
            .post_json_with_session(
                &tasks_base(&tp),
                serde_json::json!({
                    "title": format!("paging {index}"),
                    "description": "paging",
                    "status_id": status_id,
                }),
            )
            .await;
        assert_eq!(response.status(), StatusCode::CREATED, "create task");
        let body: serde_json::Value = response.json().await.expect("create json");
        created.push(body["id"].as_str().expect("task id").to_string());
    }
    // ID は作成順と無関係（UUID v4）なので、作った順のまま返っていれば食い違う
    let mut by_id = created.clone();
    by_id.sort();
    assert_ne!(by_id, created, "作成順と ID 順が偶然一致した");

    for sort in ["priority_asc", "deadline_asc"] {
        let mut read = Vec::new();
        for offset in [0, 3, 6] {
            read.extend(list_page_ids(&app, &tp, sort, 3, offset).await);
        }
        assert_eq!(read, by_id, "sort={sort}");
    }

    let mut read = Vec::new();
    for offset in [0, 3, 6] {
        read.extend(search_page_ids(&app, &tp, "paging", 3, offset).await);
    }
    assert_eq!(read, by_id, "search");

    app.cleanup_user(user.id).await;
}
