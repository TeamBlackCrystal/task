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
    for (name, color) in [
        ("bug", "#e11d48"),
        ("feature", "#3b82f6"),
        ("docs", "#10b981"),
    ] {
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

    // 置き換えは label_added / label_removed アクティビティとして 1 ラベル 1 件で記録される
    let activities_path = format!("{task_path}/activities");
    let activities = app.get_with_session(&activities_path).await;
    assert_eq!(activities.status(), StatusCode::OK);
    let activities_body: Value = activities.json().await.expect("activities json");
    let added_events: Vec<&Value> = activities_body["activities"]
        .as_array()
        .expect("activities array")
        .iter()
        .filter(|a| a["event_type"] == "label_added")
        .collect();
    let removed_events: Vec<&Value> = activities_body["activities"]
        .as_array()
        .expect("activities array")
        .iter()
        .filter(|a| a["event_type"] == "label_removed")
        .collect();
    assert_eq!(added_events.len(), 1);
    assert_eq!(added_events[0]["payload"]["label_id"], label_ids[1]);
    assert_eq!(added_events[0]["payload"]["name"], "feature");
    assert_eq!(removed_events.len(), 1);
    assert_eq!(removed_events[0]["payload"]["label_id"], label_ids[0]);
    assert_eq!(removed_events[0]["payload"]["name"], "bug");

    // 同じ集合への置き換えは記録されない
    let noop = app
        .put_json_with_session(
            &task_path,
            serde_json::json!({ "label_ids": [label_ids[1]] }),
        )
        .await;
    assert_eq!(noop.status(), StatusCode::OK);
    let activities_after_noop = app.get_with_session(&activities_path).await;
    let noop_body: Value = activities_after_noop.json().await.expect("noop json");
    let noop_events = noop_body["activities"]
        .as_array()
        .expect("activities array")
        .iter()
        .filter(|a| a["event_type"] == "label_added" || a["event_type"] == "label_removed")
        .count();
    assert_eq!(noop_events, 2);

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

    // 一括更新の add_label_ids も label_added として記録される
    let task_uuid = uuid::Uuid::parse_str(&task_id).expect("task uuid");
    let bulk_path = format!(
        "/v1/tenants/{}/projects/{}/tasks/bulk",
        tp.tenant_id, tp.project_id
    );
    let bulk_add = app
        .post_json_with_session(
            &bulk_path,
            serde_json::json!({
                "task_ids": [task_uuid],
                "update": { "add_label_ids": [label_ids[2]] }
            }),
        )
        .await;
    assert_eq!(bulk_add.status(), StatusCode::OK);
    let bulk_activities = app.get_with_session(&activities_path).await;
    let bulk_body: Value = bulk_activities.json().await.expect("bulk activities json");
    let bulk_added: Vec<&Value> = bulk_body["activities"]
        .as_array()
        .expect("activities array")
        .iter()
        .filter(|a| a["event_type"] == "label_added")
        .collect();
    // 内訳: 置き換えで feature / 再付与で bug と feature / bulk 追加で docs
    assert_eq!(bulk_added.len(), 4);
    let docs_event = bulk_added
        .iter()
        .find(|a| a["payload"]["name"] == "docs")
        .expect("bulk label_added event");
    assert_eq!(docs_event["payload"]["label_id"], label_ids[2]);

    // 既に付与済みのラベルを一括追加しても記録されない（変化なし）
    let bulk_noop = app
        .post_json_with_session(
            &bulk_path,
            serde_json::json!({
                "task_ids": [task_uuid],
                "update": { "add_label_ids": [label_ids[0]] }
            }),
        )
        .await;
    assert_eq!(bulk_noop.status(), StatusCode::OK);
    let bulk_noop_activities = app.get_with_session(&activities_path).await;
    let bulk_noop_body: Value = bulk_noop_activities
        .json()
        .await
        .expect("bulk noop activities json");
    let bulk_noop_count = bulk_noop_body["activities"]
        .as_array()
        .expect("activities array")
        .iter()
        .filter(|a| a["event_type"] == "label_added" || a["event_type"] == "label_removed")
        .count();
    // 内訳: label_added 4 件（feature / bug / feature / docs）+ label_removed 2 件（bug / feature）
    assert_eq!(bulk_noop_count, 6);

    // 一括更新の remove_label_ids でラベルを外せる（label_removed として記録される）
    let bulk_remove = app
        .post_json_with_session(
            &bulk_path,
            serde_json::json!({
                "task_ids": [task_uuid],
                "update": { "remove_label_ids": [label_ids[2]] }
            }),
        )
        .await;
    assert_eq!(bulk_remove.status(), StatusCode::OK);
    let after_remove = app.get_with_session(&task_path).await;
    let after_remove_body: Value = after_remove.json().await.expect("after remove json");
    let names: Vec<&str> = after_remove_body["labels"]
        .as_array()
        .expect("labels")
        .iter()
        .map(|l| l["name"].as_str().expect("name"))
        .collect();
    assert_eq!(names, ["bug", "feature"]);
    let remove_activities = app.get_with_session(&activities_path).await;
    let remove_body: Value = remove_activities.json().await.expect("remove json");
    let removed_events: Vec<&Value> = remove_body["activities"]
        .as_array()
        .expect("activities array")
        .iter()
        .filter(|a| a["event_type"] == "label_removed")
        .collect();
    // 内訳: 置き換えで bug と feature + 今回の bulk remove で docs
    assert_eq!(removed_events.len(), 3);
    let docs_removed = removed_events
        .iter()
        .find(|a| a["payload"]["name"] == "docs")
        .expect("bulk label_removed event");
    assert_eq!(docs_removed["payload"]["label_id"], label_ids[2]);

    // add と remove に同じ ID を含む一括更新は 400
    let conflicting = app
        .post_json_with_session(
            &bulk_path,
            serde_json::json!({
                "task_ids": [task_uuid],
                "update": {
                    "add_label_ids": [label_ids[0]],
                    "remove_label_ids": [label_ids[0]]
                }
            }),
        )
        .await;
    assert_eq!(conflicting.status(), StatusCode::BAD_REQUEST);

    // 未付与・他プロジェクトのラベル ID の remove は 200 の no-op（記録もされない）
    let harmless = app
        .post_json_with_session(
            &bulk_path,
            serde_json::json!({
                "task_ids": [task_uuid],
                "update": { "remove_label_ids": [foreign_id] }
            }),
        )
        .await;
    assert_eq!(harmless.status(), StatusCode::OK);
    let harmless_body: Value = harmless.json().await.expect("harmless json");
    assert_eq!(harmless_body["updated"], 1);
    let final_task = app.get_with_session(&task_path).await;
    let final_body: Value = final_task.json().await.expect("final json");
    assert_eq!(final_body["labels"].as_array().expect("labels").len(), 2);
    let final_activities = app.get_with_session(&activities_path).await;
    let final_activities_body: Value = final_activities.json().await.expect("final acts json");
    let final_count = final_activities_body["activities"]
        .as_array()
        .expect("activities array")
        .iter()
        .filter(|a| a["event_type"] == "label_added" || a["event_type"] == "label_removed")
        .count();
    // no-op なので直前から増えない（label_added 4 件 + label_removed 3 件）
    assert_eq!(final_count, 7);

    // 互いに異なる ID なら add と remove を 1 リクエストで同時適用できる
    // （未付与の docs を add、付与済みの bug を remove → [docs, feature]）
    let bulk_both = app
        .post_json_with_session(
            &bulk_path,
            serde_json::json!({
                "task_ids": [task_uuid],
                "update": {
                    "add_label_ids": [label_ids[2]],
                    "remove_label_ids": [label_ids[0]]
                }
            }),
        )
        .await;
    assert_eq!(bulk_both.status(), StatusCode::OK);
    let after_both = app.get_with_session(&task_path).await;
    let after_both_body: Value = after_both.json().await.expect("after both json");
    let both_names: Vec<&str> = after_both_body["labels"]
        .as_array()
        .expect("labels")
        .iter()
        .map(|l| l["name"].as_str().expect("name"))
        .collect();
    assert_eq!(both_names, ["docs", "feature"]);
    let both_activities = app.get_with_session(&activities_path).await;
    let both_body: Value = both_activities.json().await.expect("both acts json");
    let both_added: Vec<&Value> = both_body["activities"]
        .as_array()
        .expect("activities array")
        .iter()
        .filter(|a| a["event_type"] == "label_added")
        .collect();
    let both_removed: Vec<&Value> = both_body["activities"]
        .as_array()
        .expect("activities array")
        .iter()
        .filter(|a| a["event_type"] == "label_removed")
        .collect();
    // 同時適用は add 側と remove 側を両方記録する（docs の追加 / bug の削除が 1 件ずつ増える）
    assert_eq!(both_added.len(), 5);
    assert_eq!(both_removed.len(), 4);
    let docs_added = both_added
        .iter()
        .filter(|a| a["payload"]["name"] == "docs")
        .count();
    let bug_removed = both_removed
        .iter()
        .filter(|a| a["payload"]["name"] == "bug")
        .count();
    // docs は bulk 追加と今回の同時適用で 2 件、bug は最初の置き換えと今回で 2 件
    assert_eq!(docs_added, 2);
    assert_eq!(bug_removed, 2);
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
