mod common;

use axum::http::StatusCode;
use chrono::{Duration, Utc};
use common::TestApp;
use entity::task_activities;
use sea_orm::{ActiveValue::Set, EntityTrait, prelude::Uuid};
use serde_json::Value;

// アクティビティ一覧のページングの統合テスト。
//
// 履歴は操作のたびに増えるので、全件返すと長く使われたタスクほど
// DB・レスポンス・描画のコストが上限なく伸びる。既定で先頭だけ返し、
// `limit` / `offset` で段階的に取れることを固定する。

async fn json_body(res: reqwest::Response) -> Value {
    res.json::<Value>().await.expect("json body")
}

fn ids(body: &Value) -> Vec<String> {
    body["activities"]
        .as_array()
        .expect("activities array")
        .iter()
        .map(|a| a["id"].as_str().expect("id").to_string())
        .collect()
}

/// タスク作成には既定ステータスが要る。作ってその id を返す。
async fn create_default_status(app: &TestApp, tenant_id: Uuid, project_id: Uuid) -> String {
    let status_path = format!("/v1/tenants/{tenant_id}/projects/{project_id}/statuses");
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
    json_body(status).await["id"]
        .as_str()
        .expect("status id")
        .to_string()
}

/// 履歴を `count` 件積む。API 経由だと 1 件ごとに操作が要るので直接入れる。
async fn seed_activities(app: &TestApp, task_id: Uuid, user_id: Uuid, count: usize) {
    let base = Utc::now();
    let rows: Vec<task_activities::ActiveModel> = (0..count)
        .map(|i| task_activities::ActiveModel {
            id: Set(Uuid::new_v4()),
            task_id: Set(task_id),
            user_id: Set(Some(user_id)),
            event_type: Set("status_changed".into()),
            payload: Set(serde_json::json!({ "to": format!("状態 {i}") })),
            // 並びは created_at の降順なので、i が大きいほど新しくする
            created_at: Set((base + Duration::seconds(i as i64)).into()),
        })
        .collect();
    task_activities::Entity::insert_many(rows)
        .exec(&app.state.db)
        .await
        .expect("seed activities");
}

#[tokio::test]
async fn activities_are_paged_and_capped() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;

    let tasks_path = format!(
        "/v1/tenants/{}/projects/{}/tasks",
        tp.tenant_id, tp.project_id
    );

    app.reset_session_client();
    app.login_session(&owner.email, &owner.password).await;

    let status_id = create_default_status(&app, tp.tenant_id, tp.project_id).await;
    let created = app
        .post_json_with_session(
            &tasks_path,
            serde_json::json!({ "title": "履歴の多いタスク", "status_id": status_id }),
        )
        .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let created_body = json_body(created).await;
    let task_id = created_body["id"].as_str().expect("task id").to_string();
    let task_uuid = Uuid::parse_str(&task_id).expect("uuid");

    // 既定（20 件）と上限（100 件）の両方を越える件数にする。
    // 境界ちょうどだと「境界で切れるバグ」をテストが隠す
    let seeded = 137;
    seed_activities(&app, task_uuid, owner.id, seeded).await;

    let activities_path = format!("{tasks_path}/{task_id}/activities");

    // 既定は先頭 20 件。total は絞り込み前の総数（タスク作成の 1 件を含む）
    let first = app.get_with_session(&activities_path).await;
    assert_eq!(first.status(), StatusCode::OK);
    let first_body = json_body(first).await;
    let first_ids = ids(&first_body);
    assert_eq!(first_ids.len(), 20, "既定は先頭 20 件だけ返す");
    let total = first_body["total"].as_u64().expect("total");
    assert_eq!(
        total,
        seeded as u64 + 1,
        "total は返した件数ではなく総数（作成の 1 件を含む）"
    );

    // offset で続きが取れて、先頭のページと重ならない
    let second = app
        .get_with_session(&format!("{activities_path}?offset=20"))
        .await;
    assert_eq!(second.status(), StatusCode::OK);
    let second_body = json_body(second).await;
    let second_ids = ids(&second_body);
    assert_eq!(second_ids.len(), 20);
    assert!(
        second_ids.iter().all(|id| !first_ids.contains(id)),
        "続きのページが先頭と重複しない"
    );
    assert_eq!(second_body["total"].as_u64().expect("total"), total);

    // limit は上限で切る。上限を越える指定でもエラーにはしない
    let over = app
        .get_with_session(&format!("{activities_path}?limit=500"))
        .await;
    assert_eq!(over.status(), StatusCode::OK);
    assert_eq!(
        ids(&json_body(over).await).len(),
        100,
        "limit は 100 で切る"
    );

    // 末尾は残り件数だけ返る（total を越えて取ろうとしても落ちない）
    let tail = app
        .get_with_session(&format!("{activities_path}?limit=100&offset=100"))
        .await;
    assert_eq!(tail.status(), StatusCode::OK);
    assert_eq!(ids(&json_body(tail).await).len(), (total - 100) as usize);

    // 範囲外の offset は空（エラーにしない）
    let beyond = app
        .get_with_session(&format!("{activities_path}?offset={}", total + 10))
        .await;
    assert_eq!(beyond.status(), StatusCode::OK);
    let beyond_body = json_body(beyond).await;
    assert!(ids(&beyond_body).is_empty());
    assert_eq!(beyond_body["total"].as_u64().expect("total"), total);
}
