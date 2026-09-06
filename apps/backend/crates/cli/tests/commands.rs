//! コマンドが「どの URL へ何を送るか」を、モックサーバー越しに確かめる。

mod common;

use common::*;
use serde_json::json;
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

const TODO_STATUS: &str = "33333333-3333-4333-8333-333333333333";
const DONE_STATUS: &str = "66666666-6666-4666-8666-666666666666";
const ALICE_ID: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const BOB_ID: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const BUG_LABEL: &str = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
const STALE_LABEL: &str = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee";

fn label_json(id: &str, name: &str) -> serde_json::Value {
    json!({
        "id": id,
        "name": name,
        "description": "",
        "color": "#112233",
        "icon_url": null,
        "project_id": PROJECT_ID,
    })
}

fn detail_with_assignees(assignees: &[(&str, &str)]) -> serde_json::Value {
    let mut detail = task_detail_json();
    detail["assignees"] = assignees
        .iter()
        .map(|(id, username)| {
            json!({
                "role": "assignee",
                "user": { "id": id, "username": username, "avatar_url": null }
            })
        })
        .collect();
    detail
}

/// プロジェクトのキー解決と状態一覧は、ほぼ全てのコマンドの前段になる。
async fn mount_project_lookup(harness: &Harness) {
    Mock::given(method("GET"))
        .and(path(format!("/v1/tenants/{TENANT}/projects")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([project_json()])))
        .mount(&harness.server)
        .await;
}

async fn mount_statuses(harness: &Harness) {
    Mock::given(method("GET"))
        .and(path(project_path("statuses")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            status_json(TODO_STATUS, "Todo", true, false, 0),
            status_json(DONE_STATUS, "Complete", false, true, 1),
        ])))
        .mount(&harness.server)
        .await;
}

#[tokio::test]
async fn auth_whoami_reads_the_current_account() {
    let harness = harness().await;
    Mock::given(method("GET"))
        .and(path("/v1/auth/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "77777777-7777-4777-8777-777777777777",
            "username": "yupix",
            "bio": null,
            "avatar_url": null,
            "email": "yupix@example.invalid",
            "email_verified": true,
            "is_admin": false,
            "is_suspended": false,
            "totp_enabled": false,
        })))
        .expect(1)
        .mount(&harness.server)
        .await;

    assert_eq!(harness.run(&["task", "auth", "whoami"]).await.unwrap(), 0);
}

#[tokio::test]
async fn config_set_persists_the_selected_key_without_calling_the_api() {
    let harness = harness().await;

    assert_eq!(
        harness
            .run(&["task", "config", "set", "tenant_id", "tenant-2"])
            .await
            .unwrap(),
        0
    );

    assert_eq!(
        harness.store().load().unwrap().tenant_id.as_deref(),
        Some("tenant-2")
    );
    assert!(harness.sent_nothing().await);
}

#[tokio::test]
async fn config_rejects_a_key_that_is_not_part_of_the_file() {
    let harness = harness().await;
    let err = harness
        .run(&["task", "config", "get", "api-url"])
        .await
        .unwrap_err();

    assert_eq!(err.exit_code, 2);
    assert!(
        err.message.contains("Unknown config key"),
        "{}",
        err.message
    );
}

#[tokio::test]
async fn my_list_sends_its_filter() {
    let harness = harness().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/tenants/{TENANT}/users/me/tasks")))
        .and(query_param("filter", "today"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "tasks": [], "total": 0 })))
        .expect(1)
        .mount(&harness.server)
        .await;

    assert_eq!(
        harness
            .run(&["task", "my", "list", "--filter", "today"])
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn my_list_defaults_to_every_task() {
    let harness = harness().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/tenants/{TENANT}/users/me/tasks")))
        .and(query_param("filter", "all"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "tasks": [], "total": 0 })))
        .expect(1)
        .mount(&harness.server)
        .await;

    assert_eq!(harness.run(&["task", "my", "list"]).await.unwrap(), 0);
}

#[tokio::test]
async fn projects_list_reads_the_project_index() {
    let harness = harness().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/tenants/{TENANT}/projects")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([project_json()])))
        .expect(1)
        .mount(&harness.server)
        .await;

    assert_eq!(harness.run(&["task", "projects", "list"]).await.unwrap(), 0);
}

#[tokio::test]
async fn projects_show_matches_a_key_regardless_of_case() {
    let harness = harness().await;
    mount_project_lookup(&harness).await;

    assert_eq!(
        harness
            .run(&["task", "projects", "show", "app"])
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn projects_show_reports_a_key_that_does_not_exist() {
    let harness = harness().await;
    mount_project_lookup(&harness).await;

    let err = harness
        .run(&["task", "projects", "show", "NOPE"])
        .await
        .unwrap_err();
    assert_eq!(err.exit_code, 5);
    assert!(
        err.message.contains("Project not found: NOPE"),
        "{}",
        err.message
    );
}

#[tokio::test]
async fn sprints_list_builds_the_tenant_and_project_path_parameters() {
    let harness = harness().await;
    mount_project_lookup(&harness).await;
    Mock::given(method("GET"))
        .and(path(project_path("sprints")))
        .and(query_param("status", "active"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&harness.server)
        .await;

    let code = harness
        .run(&[
            "task",
            "sprints",
            "list",
            "--project",
            "APP",
            "--status",
            "active",
        ])
        .await
        .unwrap();
    assert_eq!(code, 0);
}

#[tokio::test]
async fn tasks_create_resolves_the_named_status_and_posts_it() {
    let harness = harness().await;
    mount_project_lookup(&harness).await;
    mount_statuses(&harness).await;
    Mock::given(method("POST"))
        .and(path(project_path("tasks")))
        .and(body_json(json!({
            "title": "Golden task",
            "description": null,
            "status_id": TODO_STATUS,
            "priority": "Medium",
            "progress_pct": null,
            "parent_task_id": null,
            "milestone_id": null,
            "sprint_id": null,
            "soft_deadline": null,
            "hard_deadline": null,
            "estimated_minutes": null,
            "assignees": [],
            "label_ids": [],
            "custom_field_values": [],
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(task_detail_json()))
        .expect(1)
        .mount(&harness.server)
        .await;

    let code = harness
        .run(&[
            "task",
            "tasks",
            "create",
            "--project",
            "APP",
            "--title",
            "Golden task",
            "--priority",
            "medium",
            "--status",
            "Todo",
        ])
        .await
        .unwrap();
    assert_eq!(code, 0);
}

/// 作成 API は `status_id` を必須で受ける。省略時に送らないと必ず 400 になるので、
/// プロジェクトの既定の状態で埋める。
#[tokio::test]
async fn tasks_create_falls_back_to_the_projects_default_status() {
    let harness = harness().await;
    mount_project_lookup(&harness).await;
    mount_statuses(&harness).await;
    Mock::given(method("POST"))
        .and(path(project_path("tasks")))
        .and(body_json(json!({
            "title": "No status given",
            "description": null,
            "status_id": TODO_STATUS,
            "priority": null,
            "progress_pct": null,
            "parent_task_id": null,
            "milestone_id": null,
            "sprint_id": null,
            "soft_deadline": null,
            "hard_deadline": null,
            "estimated_minutes": null,
            "assignees": [],
            "label_ids": [],
            "custom_field_values": [],
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(task_detail_json()))
        .expect(1)
        .mount(&harness.server)
        .await;

    let code = harness
        .run(&[
            "task",
            "tasks",
            "create",
            "--project",
            "APP",
            "--title",
            "No status given",
        ])
        .await
        .unwrap();
    assert_eq!(code, 0);
}

#[tokio::test]
async fn tasks_update_does_not_change_assignees_when_task_update_is_rejected() {
    let harness = harness().await;
    mount_project_lookup(&harness).await;
    Mock::given(method("GET"))
        .and(path(project_path("assignable-users")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "id": ALICE_ID, "username": "alice", "avatar_url": null }
        ])))
        .expect(1)
        .mount(&harness.server)
        .await;
    Mock::given(method("GET"))
        .and(path(project_path("tasks/APP-7")))
        .respond_with(ResponseTemplate::new(200).set_body_json(task_detail_json()))
        .expect(1)
        .mount(&harness.server)
        .await;
    Mock::given(method("PUT"))
        .and(path(project_path("tasks/APP-7")))
        .respond_with(ResponseTemplate::new(422).set_body_json(json!({
            "message": "task cannot be updated"
        })))
        .expect(1)
        .mount(&harness.server)
        .await;

    let err = harness
        .run(&[
            "task",
            "tasks",
            "update",
            "APP-7",
            "--title",
            "Rejected",
            "--assignee",
            "alice",
        ])
        .await
        .unwrap_err();

    assert_eq!(err.exit_code, 1);
    let requests = harness.server.received_requests().await.unwrap();
    assert!(requests.iter().all(|request| {
        request.method.as_str() != "POST" && request.method.as_str() != "DELETE"
    }));
}

/// `--clear-assignees` は担当者を 0 人へ置き換える。
///
/// `--assignee` は値なしを受けられないので、専用の解除が無いと今の担当者を
/// 全員外す手段が無い。今付いている全員へ DELETE が飛ぶことを見る。
#[tokio::test]
async fn tasks_update_clears_every_assignee() {
    let harness = harness().await;
    mount_project_lookup(&harness).await;
    let detail = detail_with_assignees(&[(ALICE_ID, "alice"), (BOB_ID, "bob")]);
    Mock::given(method("GET"))
        .and(path(project_path("tasks/APP-7")))
        .respond_with(ResponseTemplate::new(200).set_body_json(detail))
        // 更新前の担当者を読むときと、解除後の最終状態を読み直すときの 2 回
        .expect(2)
        .mount(&harness.server)
        .await;
    Mock::given(method("PUT"))
        .and(path(project_path("tasks/APP-7")))
        .respond_with(ResponseTemplate::new(200).set_body_json(task_detail_json()))
        .expect(1)
        .mount(&harness.server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(project_path(&format!(
            "tasks/APP-7/assignees/{ALICE_ID}"
        ))))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&harness.server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(project_path(&format!(
            "tasks/APP-7/assignees/{BOB_ID}"
        ))))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&harness.server)
        .await;

    let code = harness
        .run(&["task", "tasks", "update", "APP-7", "--clear-assignees"])
        .await
        .unwrap();

    assert_eq!(code, 0);
    let requests = harness.server.received_requests().await.unwrap();
    // 誰も足していないこと（解除だけの操作で POST は出ない）
    assert!(
        requests
            .iter()
            .all(|request| request.method.as_str() != "POST")
    );
}

#[tokio::test]
async fn tasks_update_rolls_back_assignees_when_the_sync_fails_part_way_through() {
    let harness = harness().await;
    mount_project_lookup(&harness).await;
    Mock::given(method("GET"))
        .and(path(project_path("assignable-users")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "id": ALICE_ID, "username": "alice", "avatar_url": null },
            { "id": BOB_ID, "username": "bob", "avatar_url": null }
        ])))
        .mount(&harness.server)
        .await;
    // 更新前は担当者なし。alice を付けた後は、取り直すとその 1 人が返る
    Mock::given(method("GET"))
        .and(path(project_path("tasks/APP-7")))
        .respond_with(Changing::new(vec![
            task_detail_json(),
            detail_with_assignees(&[(ALICE_ID, "alice")]),
        ]))
        .mount(&harness.server)
        .await;
    Mock::given(method("PUT"))
        .and(path(project_path("tasks/APP-7")))
        .respond_with(ResponseTemplate::new(200).set_body_json(task_detail_json()))
        .expect(1)
        .mount(&harness.server)
        .await;
    Mock::given(method("POST"))
        .and(path(project_path("tasks/APP-7/assignees")))
        .and(body_json(
            json!({ "user_id": ALICE_ID, "role": "assignee" }),
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
            "task_id": "22222222-2222-4222-8222-222222222222",
            "user_id": ALICE_ID,
            "role": "assignee",
            "assigned_at": "2026-01-01T00:00:00Z"
        })))
        .expect(1)
        .mount(&harness.server)
        .await;
    Mock::given(method("POST"))
        .and(path(project_path("tasks/APP-7/assignees")))
        .and(body_json(json!({ "user_id": BOB_ID, "role": "assignee" })))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({
            "message": "assignee service unavailable"
        })))
        .expect(1)
        .mount(&harness.server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(project_path(&format!(
            "tasks/APP-7/assignees/{ALICE_ID}"
        ))))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&harness.server)
        .await;

    let err = harness
        .run(&[
            "task",
            "tasks",
            "update",
            "APP-7",
            "--assignee",
            "alice",
            "--assignee",
            "bob",
        ])
        .await
        .unwrap_err();

    assert_eq!(err.exit_code, 1);
    assert!(err.message.contains("assignee service unavailable"));
}

/// 通信で失われた応答を「起きなかった操作」と見なすと、サーバー側で反映済みの
/// 付け外しが復元対象から漏れる。実際の担当者を取り直して戻すことを見る。
#[tokio::test]
async fn tasks_update_restores_assignees_the_server_applied_before_the_response_was_lost() {
    let harness = harness().await;
    mount_project_lookup(&harness).await;
    Mock::given(method("GET"))
        .and(path(project_path("assignable-users")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "id": ALICE_ID, "username": "alice", "avatar_url": null }
        ])))
        .mount(&harness.server)
        .await;
    // POST は失敗を返すが、サーバーは alice を付け終えている
    Mock::given(method("GET"))
        .and(path(project_path("tasks/APP-7")))
        .respond_with(Changing::new(vec![
            task_detail_json(),
            detail_with_assignees(&[(ALICE_ID, "alice")]),
        ]))
        .expect(2)
        .mount(&harness.server)
        .await;
    Mock::given(method("PUT"))
        .and(path(project_path("tasks/APP-7")))
        .respond_with(ResponseTemplate::new(200).set_body_json(task_detail_json()))
        .expect(1)
        .mount(&harness.server)
        .await;
    Mock::given(method("POST"))
        .and(path(project_path("tasks/APP-7/assignees")))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({
            "message": "assignee service unavailable"
        })))
        .expect(1)
        .mount(&harness.server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(project_path(&format!(
            "tasks/APP-7/assignees/{ALICE_ID}"
        ))))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&harness.server)
        .await;

    let err = harness
        .run(&["task", "tasks", "update", "APP-7", "--assignee", "alice"])
        .await
        .unwrap_err();

    assert_eq!(err.exit_code, 1);
    assert!(
        err.message.contains("assignees were restored"),
        "{}",
        err.message
    );
}

/// 応答が返らず実際の担当者も読めないときは、戻せたと言わない。
#[tokio::test]
async fn tasks_update_reports_unknown_assignees_when_the_task_cannot_be_read_back() {
    let harness = harness().await;
    mount_project_lookup(&harness).await;
    Mock::given(method("GET"))
        .and(path(project_path("assignable-users")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "id": ALICE_ID, "username": "alice", "avatar_url": null }
        ])))
        .mount(&harness.server)
        .await;
    Mock::given(method("GET"))
        .and(path(project_path("tasks/APP-7")))
        .respond_with(Changing::new(vec![task_detail_json()]))
        .up_to_n_times(1)
        .expect(1)
        .mount(&harness.server)
        .await;
    Mock::given(method("PUT"))
        .and(path(project_path("tasks/APP-7")))
        .respond_with(ResponseTemplate::new(200).set_body_json(task_detail_json()))
        .expect(1)
        .mount(&harness.server)
        .await;
    Mock::given(method("POST"))
        .and(path(project_path("tasks/APP-7/assignees")))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({
            "message": "assignee service unavailable"
        })))
        .expect(1)
        .mount(&harness.server)
        .await;

    let err = harness
        .run(&["task", "tasks", "update", "APP-7", "--assignee", "alice"])
        .await
        .unwrap_err();

    assert_eq!(err.exit_code, 1);
    assert!(err.message.contains("may have changed"), "{}", err.message);
    assert!(
        !err.message.contains("restored"),
        "戻せていないのに戻したと言わない: {}",
        err.message
    );
    let requests = harness.server.received_requests().await.unwrap();
    assert!(
        requests
            .iter()
            .all(|request| request.method.as_str() != "DELETE")
    );
}

/// ラベルの差分は CLI で組み直さず、一括更新 API に渡してサーバー側で当てる。
/// 読んでから全置換すると、その間に他の利用者が変えたラベルを巻き戻す。
#[tokio::test]
async fn tasks_update_sends_label_changes_as_a_diff() {
    let harness = harness().await;
    mount_project_lookup(&harness).await;
    Mock::given(method("GET"))
        .and(path(project_path("labels")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            label_json(BUG_LABEL, "bug"),
            label_json(STALE_LABEL, "stale"),
        ])))
        .mount(&harness.server)
        .await;
    Mock::given(method("PUT"))
        .and(path(project_path("tasks/APP-7")))
        .respond_with(ResponseTemplate::new(200).set_body_json(task_detail_json()))
        .expect(1)
        .mount(&harness.server)
        .await;
    Mock::given(method("POST"))
        .and(path(project_path("tasks/bulk")))
        .and(body_json(json!({
            "task_ids": ["22222222-2222-4222-8222-222222222222"],
            "update": {
                "status_id": null,
                "assignee_id": null,
                "add_label_ids": [BUG_LABEL],
                "remove_label_ids": [STALE_LABEL],
                "sprint_id": null,
                "clear_sprint_id": false,
            }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "updated": 1,
            "failed": [],
        })))
        .expect(1)
        .mount(&harness.server)
        .await;
    // 差分を当てた後の最終状態を読み直す 1 回だけ（当てる前に読まない）
    Mock::given(method("GET"))
        .and(path(project_path("tasks/APP-7")))
        .respond_with(ResponseTemplate::new(200).set_body_json(task_detail_json()))
        .expect(1)
        .mount(&harness.server)
        .await;

    let code = harness
        .run(&[
            "task",
            "tasks",
            "update",
            "APP-7",
            "--add-label",
            "bug",
            "--remove-label",
            "stale",
        ])
        .await
        .unwrap();

    assert_eq!(code, 0);
    let requests = harness.server.received_requests().await.unwrap();
    let update = requests
        .iter()
        .find(|request| request.method.as_str() == "PUT")
        .expect("本体の更新が出ている");
    let body: serde_json::Value = serde_json::from_slice(&update.body).unwrap();
    assert!(
        body["label_ids"].is_null(),
        "差分指定で全置換を送らない: {}",
        body["label_ids"]
    );
}

/// 一括更新は 1 件ごとの失敗を 200 の本文で返す。成功と取り違えない。
#[tokio::test]
async fn tasks_update_reports_a_label_change_the_bulk_endpoint_rejected() {
    let harness = harness().await;
    mount_project_lookup(&harness).await;
    Mock::given(method("GET"))
        .and(path(project_path("labels")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!([label_json(BUG_LABEL, "bug"),])),
        )
        .mount(&harness.server)
        .await;
    Mock::given(method("PUT"))
        .and(path(project_path("tasks/APP-7")))
        .respond_with(ResponseTemplate::new(200).set_body_json(task_detail_json()))
        .expect(1)
        .mount(&harness.server)
        .await;
    Mock::given(method("POST"))
        .and(path(project_path("tasks/bulk")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "updated": 0,
            "failed": [{
                "task_id": "22222222-2222-4222-8222-222222222222",
                "reason": "label is not in this project",
            }],
        })))
        .expect(1)
        .mount(&harness.server)
        .await;

    let err = harness
        .run(&["task", "tasks", "update", "APP-7", "--add-label", "bug"])
        .await
        .unwrap_err();

    assert_eq!(err.exit_code, 1);
    assert!(
        err.message.contains("label is not in this project"),
        "{}",
        err.message
    );
}

/// 一覧の絞り込みで担当者を名前で指すとき、候補の列挙（`write:task` が要る）は使わない。
/// 読むだけの PAT でも `--assignee` を名前で書けるようにする。
#[tokio::test]
async fn tasks_list_resolves_the_assignee_name_without_listing_candidates() {
    let harness = harness().await;
    mount_project_lookup(&harness).await;
    Mock::given(method("GET"))
        .and(path(project_path("assignable-users")))
        .and(query_param("username", "alice"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "id": ALICE_ID, "username": "alice", "avatar_url": null }
        ])))
        .expect(1)
        .mount(&harness.server)
        .await;
    Mock::given(method("GET"))
        .and(path(project_path("tasks")))
        .and(query_param("assignee_id", ALICE_ID))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "tasks": [], "total": 0 })))
        .expect(1)
        .mount(&harness.server)
        .await;

    let code = harness
        .run(&[
            "task",
            "tasks",
            "list",
            "--project",
            "APP",
            "--assignee",
            "alice",
        ])
        .await
        .unwrap();

    assert_eq!(code, 0);
}

/// 一覧の絞り込みはクエリ、本文は enum の綴り。取り違えるとサーバーが 400 を返す。
#[tokio::test]
async fn tasks_list_sends_the_priority_filter_in_the_query_form() {
    let harness = harness().await;
    mount_project_lookup(&harness).await;
    Mock::given(method("GET"))
        .and(path(project_path("tasks")))
        .and(query_param("priority", "critical_fire"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "tasks": [], "total": 0 })))
        .expect(1)
        .mount(&harness.server)
        .await;

    let code = harness
        .run(&[
            "task",
            "tasks",
            "list",
            "--project",
            "APP",
            "--priority",
            "critical_fire",
        ])
        .await
        .unwrap();
    assert_eq!(code, 0);
}

#[tokio::test]
async fn tasks_list_rejects_an_unknown_priority_before_sending_it() {
    let harness = harness().await;
    mount_project_lookup(&harness).await;

    let err = harness
        .run(&[
            "task",
            "tasks",
            "list",
            "--project",
            "APP",
            "--priority",
            "urgent",
        ])
        .await
        .unwrap_err();
    assert_eq!(err.exit_code, 2);
    assert!(
        err.message.starts_with("unknown priority: urgent"),
        "{}",
        err.message
    );
}

#[tokio::test]
async fn tasks_complete_moves_the_task_to_the_done_state() {
    let harness = harness().await;
    mount_project_lookup(&harness).await;
    mount_statuses(&harness).await;
    Mock::given(method("PUT"))
        .and(path(project_path("tasks/APP-7")))
        .and(body_json(json!({
            "title": null,
            "description": null,
            "clear_description": false,
            "status_id": DONE_STATUS,
            "priority": null,
            "progress_pct": null,
            "parent_task_id": null,
            "clear_parent_task_id": false,
            "milestone_id": null,
            "clear_milestone_id": false,
            "sprint_id": null,
            "clear_sprint_id": false,
            "soft_deadline": null,
            "clear_soft_deadline": false,
            "hard_deadline": null,
            "clear_hard_deadline": false,
            "estimated_minutes": null,
            "clear_estimated_minutes": false,
            "is_archived": null,
            "label_ids": null,
            "custom_field_values": null,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(task_detail_json()))
        .expect(1)
        .mount(&harness.server)
        .await;

    assert_eq!(
        harness
            .run(&["task", "tasks", "complete", "APP-7"])
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn tasks_delete_reports_success_from_a_body_less_response() {
    let harness = harness().await;
    mount_project_lookup(&harness).await;
    Mock::given(method("DELETE"))
        .and(path(project_path("tasks/APP-7")))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&harness.server)
        .await;

    assert_eq!(
        harness
            .run(&["task", "tasks", "delete", "APP-7"])
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn tasks_needs_a_project_when_the_reference_is_a_uuid() {
    let harness = harness().await;
    let err = harness
        .run(&[
            "task",
            "tasks",
            "show",
            "22222222-2222-4222-8222-222222222222",
        ])
        .await
        .unwrap_err();

    assert_eq!(err.exit_code, 2);
    assert!(
        err.message.contains("--project is required"),
        "{}",
        err.message
    );
    assert!(harness.sent_nothing().await);
}

#[tokio::test]
async fn an_expired_token_and_a_forbidden_resource_exit_with_distinct_codes() {
    for (status, expected) in [(401, 3), (403, 4), (404, 5)] {
        let harness = harness().await;
        Mock::given(method("GET"))
            .and(path("/v1/auth/me"))
            .respond_with(ResponseTemplate::new(status).set_body_json(json!({ "message": "no" })))
            .mount(&harness.server)
            .await;

        let err = harness.run(&["task", "auth", "whoami"]).await.unwrap_err();
        assert_eq!(err.exit_code, expected, "status {status}");
    }
}

#[tokio::test]
async fn a_response_that_no_longer_matches_the_shared_type_is_reported_not_swallowed() {
    let harness = harness().await;
    Mock::given(method("GET"))
        .and(path("/v1/auth/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "not-a-user" })))
        .mount(&harness.server)
        .await;

    let err = harness.run(&["task", "auth", "whoami"]).await.unwrap_err();
    assert!(
        err.message.contains("Cannot parse the API response"),
        "{}",
        err.message
    );
}
