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
    Mock::given(method("GET"))
        .and(path(project_path("tasks/APP-7")))
        .respond_with(ResponseTemplate::new(200).set_body_json(task_detail_json()))
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
