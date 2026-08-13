mod common;

use axum::http::StatusCode;
use common::TestApp;
use sea_orm::{ConnectionTrait, DatabaseBackend, QueryResult, Statement, TransactionTrait};
use serde_json::Value;
use std::time::Duration;
use uuid::Uuid;

async fn blocker_pid(txn: &sea_orm::DatabaseTransaction) -> i32 {
    txn.query_one_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        "SELECT pg_backend_pid() AS pid",
    ))
    .await
    .expect("query blocker backend pid")
    .expect("blocker backend pid row")
    .try_get("", "pid")
    .expect("blocker backend pid")
}

async fn wait_until_both_requests_are_blocked(app: &TestApp, blocker_pid: i32) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let row: QueryResult = app
                .state
                .db
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "WITH RECURSIVE blocked(pid) AS ( \
                         SELECT pid FROM pg_stat_activity \
                         WHERE $1 = ANY(pg_blocking_pids(pid)) \
                         UNION \
                         SELECT activity.pid FROM pg_stat_activity AS activity \
                         JOIN blocked ON blocked.pid = ANY(pg_blocking_pids(activity.pid)) \
                     ) SELECT COUNT(*)::bigint AS count FROM blocked",
                    [blocker_pid.into()],
                ))
                .await
                .expect("query blocked task updates")
                .expect("blocked task updates count");
            let count: i64 = row.try_get("", "count").expect("blocked request count");
            if count >= 2 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("both label replacements should wait for the task row lock");
}

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

    // label_id フィルタ: 指定ラベルが付いたタスクだけが返る
    let feature_task = app
        .post_json_with_session(
            &tasks_path,
            serde_json::json!({
                "title": "feature ラベルのタスク",
                "status_id": status_id,
                "label_ids": [label_ids[1]]
            }),
        )
        .await;
    assert_eq!(feature_task.status(), StatusCode::CREATED);
    let feature_task_body: Value = feature_task.json().await.expect("feature task json");
    let feature_task_id = feature_task_body["id"].as_str().expect("task id");

    let filtered = app
        .get_with_session(&format!("{tasks_path}?label_id={}", label_ids[1]))
        .await;
    assert_eq!(filtered.status(), StatusCode::OK);
    let filtered_body: Value = filtered.json().await.expect("filtered json");
    let filtered_tasks = filtered_body["tasks"].as_array().expect("tasks array");
    assert_eq!(filtered_tasks.len(), 2);
    assert!(
        filtered_tasks
            .iter()
            .all(|t| t["id"] == task_id.as_str() || t["id"] == feature_task_id)
    );

    let filtered_bug = app
        .get_with_session(&format!("{tasks_path}?label_id={}", label_ids[0]))
        .await;
    assert_eq!(filtered_bug.status(), StatusCode::OK);
    let filtered_bug_body: Value = filtered_bug.json().await.expect("filtered bug json");
    assert_eq!(
        filtered_bug_body["tasks"].as_array().expect("tasks").len(),
        0
    );

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

/// 同じタスクのラベル集合を並行して置換しても、二つの集合が合流しない。
#[tokio::test]
async fn concurrent_label_replacements_do_not_merge_sets() {
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
        let response = app
            .post_json_with_session(
                &labels_path,
                serde_json::json!({ "name": name, "color": color }),
            )
            .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let body: Value = response.json().await.expect("label json");
        label_ids.push(body["id"].as_str().expect("label id").to_string());
    }

    let tasks_path = format!(
        "/v1/tenants/{}/projects/{}/tasks",
        tp.tenant_id, tp.project_id
    );
    let created = app
        .post_json_with_session(
            &tasks_path,
            serde_json::json!({ "title": "並行置換", "status_id": status_id }),
        )
        .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let created_body: Value = created.json().await.expect("created task json");
    let task_id = created_body["id"].as_str().expect("task id").to_string();
    let task_uuid: Uuid = task_id.parse().expect("task uuid");

    // NO KEY UPDATE は task_labels の外部キー検査を妨げず、親タスクの UPDATE と
    // SELECT FOR UPDATE だけを待機させる。旧実装なら両方の INSERT が先に完了して
    // 和集合となり、修正後は両方が DELETE 前にここで待機する。
    let blocker = app
        .state
        .db
        .begin()
        .await
        .expect("begin blocker transaction");
    blocker
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM tasks WHERE id = $1 FOR NO KEY UPDATE",
            [task_uuid.into()],
        ))
        .await
        .expect("lock task row")
        .expect("locked task row");
    let blocker_pid = blocker_pid(&blocker).await;

    let task_url = format!("{}{tasks_path}/{task_id}", app.base_url());
    let first_client = app.client().clone();
    let first_url = task_url.clone();
    let first_label_id = label_ids[0].clone();
    let first = tokio::spawn(async move {
        first_client
            .put(first_url)
            .json(&serde_json::json!({ "label_ids": [first_label_id] }))
            .send()
            .await
            .expect("first concurrent replacement")
    });
    let second_client = app.client().clone();
    let second_label_id = label_ids[1].clone();
    let second = tokio::spawn(async move {
        second_client
            .put(task_url)
            .json(&serde_json::json!({ "label_ids": [second_label_id] }))
            .send()
            .await
            .expect("second concurrent replacement")
    });

    wait_until_both_requests_are_blocked(&app, blocker_pid).await;
    blocker.commit().await.expect("release task row lock");

    let first_response = first.await.expect("join first replacement");
    let second_response = second.await.expect("join second replacement");
    assert_eq!(first_response.status(), StatusCode::OK);
    assert_eq!(second_response.status(), StatusCode::OK);

    let final_response = app
        .get_with_session(&format!("{tasks_path}/{task_id}"))
        .await;
    assert_eq!(final_response.status(), StatusCode::OK);
    let final_body: Value = final_response.json().await.expect("final task json");
    let final_labels = final_body["labels"].as_array().expect("final labels");
    assert_eq!(
        final_labels.len(),
        1,
        "concurrent replacements must not merge"
    );
    let final_id = final_labels[0]["id"].as_str().expect("final label id");
    assert!(label_ids.iter().any(|id| id == final_id));
}
