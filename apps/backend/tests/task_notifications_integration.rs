mod common;

use axum::http::StatusCode;
use common::TestApp;
use serde_json::Value;

#[tokio::test]
async fn task_notifications_integration_suite() {
    let mut app = TestApp::new().await;
    let owner = app.insert_user(false, false).await;
    app.login_session_no_content(&owner.email, &owner.password)
        .await;
    let tp = app.insert_tenant_project(owner.id).await;

    let status_path = format!(
        "/v1/tenants/{}/projects/{}/statuses",
        tp.tenant_id, tp.project_id
    );
    let status_resp = app
        .post_json_with_session(
            &status_path,
            serde_json::json!({"name":"Backlog","color":"#336699","position":0,"is_default":true}),
        )
        .await;
    assert_eq!(status_resp.status(), StatusCode::CREATED);
    let status_id = status_resp.json::<serde_json::Value>().await.expect("json")["id"]
        .as_str()
        .unwrap()
        .to_string();

    let tasks_path = format!(
        "/v1/tenants/{}/projects/{}/tasks",
        tp.tenant_id, tp.project_id
    );
    let task_resp = app
        .post_json_with_session(
            &tasks_path,
            serde_json::json!({"title":"Notify task","status_id":status_id}),
        )
        .await;
    assert_eq!(task_resp.status(), StatusCode::CREATED);
    let task_id = task_resp.json::<serde_json::Value>().await.expect("json")["id"]
        .as_str()
        .unwrap()
        .to_string();

    let assignee = app.insert_user(false, false).await;
    let assignee_username = format!("test_{}", &assignee.id.to_string()[..8]);

    common::ensure_tenant_member_for_project(&app.state.db, tp.project_id, assignee.id).await;
    let member_resp = app
        .post_json_with_session(
            &format!(
                "/v1/tenants/{}/projects/{}/members",
                tp.tenant_id, tp.project_id
            ),
            serde_json::json!({"user_id": assignee.id, "role": "Member"}),
        )
        .await;
    assert_eq!(member_resp.status(), StatusCode::CREATED);

    let task_base = format!(
        "/v1/tenants/{}/projects/{}/tasks/{}",
        tp.tenant_id, tp.project_id, task_id
    );
    let assign_resp = app
        .post_json_with_session(
            &format!("{task_base}/assignees"),
            serde_json::json!({"user_id": assignee.id, "role": "primary"}),
        )
        .await;
    assert_eq!(assign_resp.status(), StatusCode::CREATED);

    let watchers = app.get_with_session(&format!("{task_base}/watchers")).await;
    assert_eq!(watchers.status(), StatusCode::OK);
    assert_eq!(
        watchers.json::<serde_json::Value>().await.expect("json")["watchers"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    app.reset_session_client();
    app.login_session_no_content(&assignee.email, &assignee.password)
        .await;
    let notif = app.get_with_session("/v1/users/me/notifications").await;
    assert_eq!(notif.status(), StatusCode::OK);
    let body: Value = notif.json::<serde_json::Value>().await.expect("json");
    assert_eq!(body["unread_count"].as_u64(), Some(1));

    app.reset_session_client();
    app.login_session_no_content(&owner.email, &owner.password)
        .await;
    assert_eq!(
        app.post_json_with_session(&format!("{task_base}/watch"), serde_json::json!({}))
            .await
            .status(),
        StatusCode::CREATED
    );
    let comment = app
        .post_json_with_session(
            &format!("{task_base}/comments"),
            serde_json::json!({"body": format!("@{assignee_username} review")}),
        )
        .await;
    assert_eq!(comment.status(), StatusCode::CREATED);

    app.reset_session_client();
    app.login_session_no_content(&assignee.email, &assignee.password)
        .await;
    let unread = app
        .get_with_session("/v1/users/me/notifications?unread=true")
        .await;
    let unread_body: Value = unread.json::<Value>().await.expect("json");
    let types: Vec<&str> = unread_body["notifications"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|n| n["notification_type"].as_str())
        .collect();
    assert!(types.contains(&"mentioned"));
}

#[tokio::test]
async fn watcher_manual_watch_and_unwatch() {
    let mut app = TestApp::new().await;
    let owner = app.insert_user(false, false).await;
    app.login_session_no_content(&owner.email, &owner.password)
        .await;
    let tp = app.insert_tenant_project(owner.id).await;

    let status_resp = app
        .post_json_with_session(
            &format!(
                "/v1/tenants/{}/projects/{}/statuses",
                tp.tenant_id, tp.project_id
            ),
            serde_json::json!({"name":"Todo","color":"#aabbcc","position":0,"is_default":true}),
        )
        .await;
    let status_id = status_resp.json::<Value>().await.expect("json")["id"]
        .as_str()
        .unwrap()
        .to_string();

    let task_resp = app
        .post_json_with_session(
            &format!(
                "/v1/tenants/{}/projects/{}/tasks",
                tp.tenant_id, tp.project_id
            ),
            serde_json::json!({"title":"Watch test","status_id":status_id}),
        )
        .await;
    let task_id = task_resp.json::<Value>().await.expect("json")["id"]
        .as_str()
        .unwrap()
        .to_string();
    let task_base = format!(
        "/v1/tenants/{}/projects/{}/tasks/{}",
        tp.tenant_id, tp.project_id, task_id
    );

    let watcher = app.insert_user(false, false).await;
    common::ensure_tenant_member_for_project(&app.state.db, tp.project_id, watcher.id).await;
    app.post_json_with_session(
        &format!(
            "/v1/tenants/{}/projects/{}/members",
            tp.tenant_id, tp.project_id
        ),
        serde_json::json!({"user_id": watcher.id, "role": "Member"}),
    )
    .await;

    // 手動ウォッチ
    app.reset_session_client();
    app.login_session_no_content(&watcher.email, &watcher.password)
        .await;
    let watch_resp = app
        .post_json_with_session(&format!("{task_base}/watch"), serde_json::json!({}))
        .await;
    assert_eq!(watch_resp.status(), StatusCode::CREATED);

    // ウォッチャー一覧に追加されていることを確認
    app.reset_session_client();
    app.login_session_no_content(&owner.email, &owner.password)
        .await;
    let watchers_resp = app.get_with_session(&format!("{task_base}/watchers")).await;
    let watchers_body: Value = watchers_resp.json().await.expect("json");
    let watcher_ids: Vec<&str> = watchers_body["watchers"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|w| w["id"].as_str())
        .collect();
    assert!(watcher_ids.contains(&watcher.id.to_string().as_str()));

    // ウォッチ解除
    app.reset_session_client();
    app.login_session_no_content(&watcher.email, &watcher.password)
        .await;
    let unwatch_resp = app.delete_with_session(&format!("{task_base}/watch")).await;
    assert_eq!(unwatch_resp.status(), StatusCode::NO_CONTENT);

    // ウォッチャー一覧から削除されていることを確認
    app.reset_session_client();
    app.login_session_no_content(&owner.email, &owner.password)
        .await;
    let watchers_resp2 = app.get_with_session(&format!("{task_base}/watchers")).await;
    let watchers_body2: Value = watchers_resp2.json().await.expect("json");
    let watcher_ids2: Vec<&str> = watchers_body2["watchers"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|w| w["id"].as_str())
        .collect();
    assert!(!watcher_ids2.contains(&watcher.id.to_string().as_str()));
}

#[tokio::test]
async fn mark_notification_read_and_read_all() {
    let mut app = TestApp::new().await;
    let owner = app.insert_user(false, false).await;
    app.login_session_no_content(&owner.email, &owner.password)
        .await;
    let tp = app.insert_tenant_project(owner.id).await;

    let status_resp = app
        .post_json_with_session(
            &format!(
                "/v1/tenants/{}/projects/{}/statuses",
                tp.tenant_id, tp.project_id
            ),
            serde_json::json!({"name":"Todo","color":"#aabbcc","position":0,"is_default":true}),
        )
        .await;
    let status_id = status_resp.json::<Value>().await.expect("json")["id"]
        .as_str()
        .unwrap()
        .to_string();

    let assignee = app.insert_user(false, false).await;
    common::ensure_tenant_member_for_project(&app.state.db, tp.project_id, assignee.id).await;
    app.post_json_with_session(
        &format!(
            "/v1/tenants/{}/projects/{}/members",
            tp.tenant_id, tp.project_id
        ),
        serde_json::json!({"user_id": assignee.id, "role": "Member"}),
    )
    .await;

    // 2件通知を生成（担当者追加×2タスク）
    for title in ["Task A", "Task B"] {
        let task_resp = app
            .post_json_with_session(
                &format!(
                    "/v1/tenants/{}/projects/{}/tasks",
                    tp.tenant_id, tp.project_id
                ),
                serde_json::json!({"title": title, "status_id": status_id}),
            )
            .await;
        let task_id = task_resp.json::<Value>().await.expect("json")["id"]
            .as_str()
            .unwrap()
            .to_string();
        app.post_json_with_session(
            &format!(
                "/v1/tenants/{}/projects/{}/tasks/{}/assignees",
                tp.tenant_id, tp.project_id, task_id
            ),
            serde_json::json!({"user_id": assignee.id, "role": "primary"}),
        )
        .await;
    }

    app.reset_session_client();
    app.login_session_no_content(&assignee.email, &assignee.password)
        .await;

    let notifs: Value = app
        .get_with_session("/v1/users/me/notifications")
        .await
        .json()
        .await
        .expect("json");
    assert_eq!(notifs["unread_count"].as_u64(), Some(2));
    let notif_id = notifs["notifications"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // 1件既読
    let read_resp = app
        .patch_json_with_session(
            &format!("/v1/users/me/notifications/{}/read", notif_id),
            serde_json::json!({}),
        )
        .await;
    assert_eq!(read_resp.status(), StatusCode::OK);
    let read_body: Value = read_resp.json().await.expect("json");
    assert!(read_body["read_at"].as_str().is_some());

    let notifs2: Value = app
        .get_with_session("/v1/users/me/notifications")
        .await
        .json()
        .await
        .expect("json");
    assert_eq!(notifs2["unread_count"].as_u64(), Some(1));

    // 全件既読
    let all_read = app
        .patch_json_with_session("/v1/users/me/notifications/read-all", serde_json::json!({}))
        .await;
    assert_eq!(all_read.status(), StatusCode::NO_CONTENT);

    let notifs3: Value = app
        .get_with_session("/v1/users/me/notifications")
        .await
        .json()
        .await
        .expect("json");
    assert_eq!(notifs3["unread_count"].as_u64(), Some(0));
}

#[tokio::test]
async fn status_changed_notification_to_watcher() {
    let mut app = TestApp::new().await;
    let owner = app.insert_user(false, false).await;
    app.login_session_no_content(&owner.email, &owner.password)
        .await;
    let tp = app.insert_tenant_project(owner.id).await;

    let status_path = format!(
        "/v1/tenants/{}/projects/{}/statuses",
        tp.tenant_id, tp.project_id
    );
    let status_a_id = app
        .post_json_with_session(
            &status_path,
            serde_json::json!({"name":"Todo","color":"#aaaaaa","position":0,"is_default":true}),
        )
        .await
        .json::<Value>()
        .await
        .expect("json")["id"]
        .as_str()
        .unwrap()
        .to_string();
    let status_b_id = app
        .post_json_with_session(
            &status_path,
            serde_json::json!({"name":"Done","color":"#bbbbbb","position":1,"is_default":false}),
        )
        .await
        .json::<Value>()
        .await
        .expect("json")["id"]
        .as_str()
        .unwrap()
        .to_string();

    let watcher = app.insert_user(false, false).await;
    common::ensure_tenant_member_for_project(&app.state.db, tp.project_id, watcher.id).await;
    app.post_json_with_session(
        &format!(
            "/v1/tenants/{}/projects/{}/members",
            tp.tenant_id, tp.project_id
        ),
        serde_json::json!({"user_id": watcher.id, "role": "Member"}),
    )
    .await;

    let task_id = app
        .post_json_with_session(
            &format!(
                "/v1/tenants/{}/projects/{}/tasks",
                tp.tenant_id, tp.project_id
            ),
            serde_json::json!({"title":"Status test","status_id":status_a_id}),
        )
        .await
        .json::<Value>()
        .await
        .expect("json")["id"]
        .as_str()
        .unwrap()
        .to_string();
    let task_base = format!(
        "/v1/tenants/{}/projects/{}/tasks/{}",
        tp.tenant_id, tp.project_id, task_id
    );

    // ウォッチャーをタスクに追加
    app.post_json_with_session(
        &format!(
            "/v1/tenants/{}/projects/{}/tasks/{}/assignees",
            tp.tenant_id, tp.project_id, task_id
        ),
        serde_json::json!({"user_id": watcher.id, "role": "primary"}),
    )
    .await;

    // ステータス変更（owner が実行）
    let update_resp = app
        .put_json_with_session(&task_base, serde_json::json!({"status_id": status_b_id}))
        .await;
    assert_eq!(update_resp.status(), StatusCode::OK);

    // ウォッチャーに status_changed 通知が届いていることを確認
    app.reset_session_client();
    app.login_session_no_content(&watcher.email, &watcher.password)
        .await;
    let notifs: Value = app
        .get_with_session("/v1/users/me/notifications")
        .await
        .json()
        .await
        .expect("json");
    let types: Vec<&str> = notifs["notifications"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|n| n["notification_type"].as_str())
        .collect();
    assert!(
        types.contains(&"status_changed"),
        "status_changed notification not found: {:?}",
        types
    );
}

#[tokio::test]
async fn notification_settings_get_update_and_validation() {
    let mut app = TestApp::new().await;
    let owner = app.insert_user(false, false).await;
    app.login_session_no_content(&owner.email, &owner.password)
        .await;
    let tp = app.insert_tenant_project(owner.id).await;
    let settings_path = format!("/v1/users/me/notification-settings/{}", tp.project_id);

    // デフォルト設定の取得
    let defaults: Value = app
        .get_with_session(&settings_path)
        .await
        .json()
        .await
        .expect("json");
    assert!(
        defaults["in_app_events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e.as_str() == Some("assigned"))
    );

    // 有効な設定の更新
    let update_resp = app
        .put_json_with_session(
            &settings_path,
            serde_json::json!({
                "email_events": [],
                "in_app_events": ["assigned", "mentioned"]
            }),
        )
        .await;
    assert_eq!(update_resp.status(), StatusCode::OK);
    let updated: Value = update_resp.json().await.expect("json");
    assert_eq!(updated["in_app_events"].as_array().unwrap().len(), 2);

    // 変更が永続化されていることを確認
    let fetched: Value = app
        .get_with_session(&settings_path)
        .await
        .json()
        .await
        .expect("json");
    assert_eq!(fetched["in_app_events"], updated["in_app_events"]);

    // 不正なイベントタイプ → 422
    let invalid_resp = app
        .put_json_with_session(
            &settings_path,
            serde_json::json!({
                "email_events": [],
                "in_app_events": ["assigned", "invalid_event_xyz"]
            }),
        )
        .await;
    assert_eq!(invalid_resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn mention_notifies_tenant_owner_non_member() {
    let mut app = TestApp::new().await;

    // owner（tenant owner、project member にはしない）
    let owner = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;
    let owner_username = format!("test_{}", &owner.id.to_string()[..8]);

    // project member を作成
    let member = app.insert_user(false, false).await;

    // owner でログインして member を project に追加
    app.login_session_no_content(&owner.email, &owner.password)
        .await;
    common::ensure_tenant_member_for_project(&app.state.db, tp.project_id, member.id).await;
    let member_resp = app
        .post_json_with_session(
            &format!(
                "/v1/tenants/{}/projects/{}/members",
                tp.tenant_id, tp.project_id
            ),
            serde_json::json!({"user_id": member.id, "role": "Member"}),
        )
        .await;
    assert_eq!(member_resp.status(), StatusCode::CREATED);

    // ステータスとタスクを作成
    let status_resp = app
        .post_json_with_session(
            &format!(
                "/v1/tenants/{}/projects/{}/statuses",
                tp.tenant_id, tp.project_id
            ),
            serde_json::json!({"name":"Backlog","color":"#336699","position":0,"is_default":true}),
        )
        .await;
    assert_eq!(status_resp.status(), StatusCode::CREATED);
    let status_id = status_resp.json::<Value>().await.expect("json")["id"]
        .as_str()
        .unwrap()
        .to_string();

    let task_resp = app
        .post_json_with_session(
            &format!(
                "/v1/tenants/{}/projects/{}/tasks",
                tp.tenant_id, tp.project_id
            ),
            serde_json::json!({"title":"Mention owner test","status_id":status_id}),
        )
        .await;
    assert_eq!(task_resp.status(), StatusCode::CREATED);
    let task_id = task_resp.json::<Value>().await.expect("json")["id"]
        .as_str()
        .unwrap()
        .to_string();

    // member でログインして @owner_username を含むコメントを投稿
    app.reset_session_client();
    app.login_session_no_content(&member.email, &member.password)
        .await;
    let comment_resp = app
        .post_json_with_session(
            &format!(
                "/v1/tenants/{}/projects/{}/tasks/{}/comments",
                tp.tenant_id, tp.project_id, task_id
            ),
            serde_json::json!({"body": format!("@{} please review", owner_username)}),
        )
        .await;
    assert_eq!(comment_resp.status(), StatusCode::CREATED);

    // owner でログインして通知を確認
    app.reset_session_client();
    app.login_session_no_content(&owner.email, &owner.password)
        .await;
    let notifs: Value = app
        .get_with_session("/v1/users/me/notifications")
        .await
        .json()
        .await
        .expect("json");

    let types: Vec<&str> = notifs["notifications"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|n| n["notification_type"].as_str())
        .collect();
    assert!(
        types.contains(&"mentioned"),
        "tenant owner should receive mention notification even if not a project member, got: {:?}",
        types
    );
}
