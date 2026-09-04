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

/// 同時刻のタスクがページ境界をまたいでも、重複も欠落もしない。
///
/// List 表示はステータスごとに `offset` を 20 件ずつ増やしてページを継ぐ。既定の
/// 並びは `created_at DESC` だけだったので、同じ `created_at` を持つタスクが境界に
/// 来ると、静的なデータへの連続リクエストでも同じタスクが複数ページに出たり、
/// 別のタスクがどのページにも出なかったりする。frontend の `toTaskGroup()` は重複 ID を
/// 落とすので重複は隠れるが、欠落は戻らない。
#[tokio::test]
async fn task_list_paging_is_stable_when_created_at_ties() {
    let mut app = TestApp::new().await;
    let (_user, tp) = setup_project(&mut app).await;
    let status_id = create_status(&app, &tp).await;
    let base = tasks_base(&tp);

    // ページサイズ（20）を越える件数を、すべて同じ created_at で作る。
    // 作成 API は now() を入れるので、作ってから DB 側で揃える
    let count = 47;
    let mut created = Vec::new();
    for i in 0..count {
        let response = app
            .post_json_with_session(
                &base,
                serde_json::json!({ "title": format!("同時刻 {i}"), "status_id": status_id }),
            )
            .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let body: serde_json::Value = response.json().await.expect("task json");
        created.push(body["id"].as_str().expect("id").to_string());
    }
    common::execute_sql(
        &app.state.db,
        "UPDATE tasks SET created_at = now() WHERE project_id = $1",
        vec![tp.project_id.into()],
    )
    .await;

    let mut seen: Vec<String> = Vec::new();
    let mut offset = 0;
    loop {
        let page = app
            .get_with_session(&format!("{base}?limit=20&offset={offset}"))
            .await;
        assert_eq!(page.status(), StatusCode::OK);
        let body: serde_json::Value = page.json().await.expect("list json");
        let ids: Vec<String> = body["tasks"]
            .as_array()
            .expect("tasks array")
            .iter()
            .map(|t| t["id"].as_str().expect("id").to_string())
            .collect();
        if ids.is_empty() {
            break;
        }
        seen.extend(ids);
        offset += 20;
    }

    assert_eq!(
        seen.len(),
        count,
        "ページを跨いで欠落・重複している: {} 件しか取れていない",
        seen.len()
    );
    let unique: std::collections::HashSet<&String> = seen.iter().collect();
    assert_eq!(unique.len(), count, "同じタスクが複数ページに出ている");

    // **並びが全順序になっていることを直接見る。**
    // 「ページを継いで欠落しない」だけでは、たまたま安定した実行計画のときに
    // タイブレーカーが無くても通ってしまう（seq scan の物理順で返るため）。
    // created_at が全件同じなので、id の降順になっていれば
    // `created_at DESC, id DESC` が効いている。
    let mut expected = seen.clone();
    expected.sort_by(|a, b| b.cmp(a));
    assert_eq!(
        seen, expected,
        "created_at が同じ行が id 降順に並んでいない（タイブレーカーが効いていない）"
    );
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
