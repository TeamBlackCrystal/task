mod common;

use axum::http::StatusCode;
use common::{TestApp, TestTenantProject, insert_user};
use entity::{project_statuses, task_timers, tasks, time_logs};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

struct TaskFixture {
    tp: TestTenantProject,
    task_id: Uuid,
    #[allow(dead_code)]
    status_id: Uuid,
}

async fn setup_task_fixture(app: &TestApp, owner_id: Uuid) -> TaskFixture {
    let tp = app.insert_tenant_project(owner_id).await;
    let status_id = Uuid::new_v4();
    project_statuses::ActiveModel {
        id: Set(status_id),
        project_id: Set(tp.project_id),
        name: Set("Todo".into()),
        color: Set("#808080".into()),
        position: Set(0),
        is_default: Set(true),
        is_done_state: Set(false),
        created_at: Set(chrono::Utc::now().into()),
    }
    .insert(&app.state.db)
    .await
    .expect("insert status");

    let task_id = Uuid::new_v4();
    tasks::ActiveModel {
        id: Set(task_id),
        project_id: Set(tp.project_id),
        seq_id: Set(1),
        title: Set("Time tracking test".into()),
        description: Set(None),
        status_id: Set(status_id),
        priority: Set(entity::tasks::TaskPriority::Medium),
        progress_pct: Set(0),
        parent_task_id: Set(None),
        milestone_id: Set(None),
        soft_deadline: Set(None),
        hard_deadline: Set(None),
        estimated_minutes: Set(Some(180)),
        is_archived: Set(false),
        completed_at: Set(None),
        sprint_id: Set(None),
        created_by: Set(owner_id),
        created_at: Set(chrono::Utc::now().into()),
        updated_at: Set(chrono::Utc::now().into()),
        deleted_at: Set(None),
    }
    .insert(&app.state.db)
    .await
    .expect("insert task");

    TaskFixture {
        tp,
        task_id,
        status_id,
    }
}

fn task_base_path(tp: &TestTenantProject, task_id: Uuid) -> String {
    format!(
        "/v1/tenants/{}/projects/{}/tasks/{}",
        tp.tenant_id, tp.project_id, task_id
    )
}

#[tokio::test]
async fn time_tracking_integration_suite() {
    let mut app = TestApp::new().await;
    let owner = app.insert_user_default().await;
    let fixture = setup_task_fixture(&app, owner.id).await;
    let base = task_base_path(&fixture.tp, fixture.task_id);

    app.reset_session_client();
    app.login_session_no_content(&owner.email, &owner.password)
        .await;

    // 1. 手動ログ追加
    let create = app
        .post_json_with_session(
            &format!("{base}/time-logs"),
            serde_json::json!({
                "logged_minutes": 90,
                "logged_at": "2026-05-27",
                "note": "設計レビュー対応"
            }),
        )
        .await;
    assert_eq!(create.status(), StatusCode::CREATED);
    let created: serde_json::Value = create.json().await.expect("create json");
    let log_id = created["id"]
        .as_str()
        .expect("log id")
        .parse::<Uuid>()
        .unwrap();
    assert_eq!(created["logged_minutes"], 90);

    // 2. ログ一覧
    let list = app.get_with_session(&format!("{base}/time-logs")).await;
    assert_eq!(list.status(), StatusCode::OK);
    let logs: Vec<serde_json::Value> = list.json().await.expect("list json");
    assert_eq!(logs.len(), 1);

    // 3. サマリー
    let summary = app
        .get_with_session(&format!("{base}/time-logs/summary"))
        .await;
    assert_eq!(summary.status(), StatusCode::OK);
    let summary_body: serde_json::Value = summary.json().await.expect("summary json");
    assert_eq!(summary_body["estimated_minutes"], 180);
    assert_eq!(summary_body["actual_minutes"], 90);
    assert_eq!(summary_body["remaining_minutes"], 90);
    assert_eq!(summary_body["is_over"], false);

    // 4. ログ編集
    let update = app
        .client()
        .put(format!("{}{base}/time-logs/{log_id}", app.base_url()))
        .json(&serde_json::json!({
            "logged_minutes": 60,
            "note": "更新後"
        }))
        .send()
        .await
        .expect("put log");
    assert_eq!(update.status(), StatusCode::OK);
    let updated: serde_json::Value = update.json().await.expect("update json");
    assert_eq!(updated["logged_minutes"], 60);

    // 5. タイマー start → status → stop
    let start = app
        .post_json_with_session(&format!("{base}/timer/start"), serde_json::json!({}))
        .await;
    assert_eq!(start.status(), StatusCode::CREATED);

    let status = app.get_with_session(&format!("{base}/timer/status")).await;
    assert_eq!(status.status(), StatusCode::OK);
    let status_body: serde_json::Value = status.json().await.expect("status json");
    assert_eq!(status_body["is_running"], true);

    let dup_start = app
        .post_json_with_session(&format!("{base}/timer/start"), serde_json::json!({}))
        .await;
    assert_eq!(dup_start.status(), StatusCode::CONFLICT);

    let stop = app
        .post_json_with_session(&format!("{base}/timer/stop"), serde_json::json!({}))
        .await;
    assert_eq!(stop.status(), StatusCode::OK);
    let stop_body: serde_json::Value = stop.json().await.expect("stop json");
    assert!(stop_body["logged_minutes"].as_i64().unwrap_or(0) >= 1);

    let timer_row = task_timers::Entity::find()
        .filter(task_timers::Column::TaskId.eq(fixture.task_id))
        .filter(task_timers::Column::UserId.eq(owner.id))
        .one(&app.state.db)
        .await
        .expect("query timer");
    assert!(timer_row.is_none());

    // 6. 権限: 非メンバーは 403
    let outsider = insert_user(&app.state.db, false, false).await;
    app.reset_session_client();
    app.login_session_no_content(&outsider.email, &outsider.password)
        .await;
    let forbidden = app.get_with_session(&format!("{base}/time-logs")).await;
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
    app.cleanup_user(outsider.id).await;

    // 7. 他人のログ編集は 403（メンバーとして追加）
    let member = insert_user(&app.state.db, false, false).await;
    common::ensure_tenant_member_for_project(&app.state.db, fixture.tp.project_id, member.id).await;
    entity::project_members::ActiveModel {
        id: Set(Uuid::new_v4()),
        project_id: Set(fixture.tp.project_id),
        user_id: Set(member.id),
        role: Set(entity::project_members::ProjectRole::Member),
    }
    .insert(&app.state.db)
    .await
    .expect("add member");

    app.reset_session_client();
    app.login_session_no_content(&member.email, &member.password)
        .await;
    let edit_other = app
        .client()
        .put(format!("{}{base}/time-logs/{log_id}", app.base_url()))
        .json(&serde_json::json!({ "logged_minutes": 10 }))
        .send()
        .await
        .expect("edit other");
    assert_eq!(edit_other.status(), StatusCode::FORBIDDEN);

    // 8. テナントオーナーは他人のログ削除可
    app.reset_session_client();
    app.login_session_no_content(&owner.email, &owner.password)
        .await;
    let delete = app
        .delete_with_session(&format!("{base}/time-logs/{log_id}"))
        .await;
    assert_eq!(delete.status(), StatusCode::NO_CONTENT);

    let remaining = time_logs::Entity::find_by_id(log_id)
        .one(&app.state.db)
        .await
        .expect("query log");
    assert!(remaining.is_none());

    // 9. 異常系: 存在しないログ
    let missing = app
        .delete_with_session(&format!("{base}/time-logs/{}", Uuid::new_v4()))
        .await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    // 10. 異常系: logged_minutes <= 0
    let bad = app
        .post_json_with_session(
            &format!("{base}/time-logs"),
            serde_json::json!({
                "logged_minutes": 0,
                "logged_at": "2026-05-27"
            }),
        )
        .await;
    assert!(
        bad.status() == StatusCode::UNPROCESSABLE_ENTITY || bad.status() == StatusCode::BAD_REQUEST,
        "invalid logged_minutes should be rejected, got {}",
        bad.status()
    );

    app.cleanup_user(member.id).await;
    app.cleanup_user(owner.id).await;
}
