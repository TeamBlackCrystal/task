mod common;

use axum::http::StatusCode;
use chrono::{DateTime, Duration, Utc};
use common::TestApp;
use entity::task_activities;
use sea_orm::{ActiveValue::Set, EntityTrait, prelude::Uuid};
use serde_json::Value;

// アクティビティ一覧のページングの統合テスト。
//
// 履歴は操作のたびに増えるので、全件返すと長く使われたタスクほど
// DB・レスポンス・描画のコストが上限なく伸びる。既定で先頭だけ返し、
// `limit` / `cursor` で段階的に取れることを固定する。
//
// 継ぎ目に offset を使わないのは、履歴が積まれている最中にページを継ぐと
// 境界がずれ、同じ行が 2 度出たり抜けたりするため。

async fn json_body(res: reqwest::Response) -> Value {
    res.json::<Value>().await.expect("json body")
}

/// 次のページの鍵。取り切っていれば `None`。
fn next_cursor(body: &Value) -> Option<String> {
    body["next_cursor"].as_str().map(|s| s.to_string())
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
async fn seed_activities(
    app: &TestApp,
    task_id: Uuid,
    user_id: Uuid,
    count: usize,
    base: DateTime<Utc>,
) {
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

/// 履歴を `count` 件、**すべて同じ `created_at`** で積む。
///
/// 実際の履歴は同一トランザクションの連続操作で同時刻になりうる。並び順に
/// タイブレーカーが無いと、同時刻の行がページ境界に来た時点で重複・欠落が出る。
async fn seed_activities_at_same_instant(
    app: &TestApp,
    task_id: Uuid,
    user_id: Uuid,
    count: usize,
) {
    let at = Utc::now();
    let rows: Vec<task_activities::ActiveModel> = (0..count)
        .map(|i| task_activities::ActiveModel {
            id: Set(Uuid::new_v4()),
            task_id: Set(task_id),
            user_id: Set(Some(user_id)),
            event_type: Set("status_changed".into()),
            payload: Set(serde_json::json!({ "to": format!("同時刻 {i}") })),
            created_at: Set(at.into()),
        })
        .collect();
    task_activities::Entity::insert_many(rows)
        .exec(&app.state.db)
        .await
        .expect("seed activities");
}

/// 同時刻の履歴がページ境界をまたいでも、重複も欠落もしない。
#[tokio::test]
async fn activities_paging_is_stable_when_timestamps_tie() {
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
            serde_json::json!({ "title": "同時刻の履歴", "status_id": status_id }),
        )
        .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let task_id = json_body(created).await["id"]
        .as_str()
        .expect("task id")
        .to_string();
    let task_uuid = Uuid::parse_str(&task_id).expect("uuid");

    // 既定の 1 ページ（20 件）を越える件数を同一時刻で積み、境界に同時刻を必ず置く
    let seeded = 47;
    seed_activities_at_same_instant(&app, task_uuid, owner.id, seeded).await;

    let activities_path = format!("{tasks_path}/{task_id}/activities");
    let mut seen: Vec<String> = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let url = match &cursor {
            Some(c) => format!("{activities_path}?limit=20&cursor={c}"),
            None => format!("{activities_path}?limit=20"),
        };
        let page = app.get_with_session(&url).await;
        assert_eq!(page.status(), StatusCode::OK);
        let body = json_body(page).await;
        seen.extend(ids(&body));
        match next_cursor(&body) {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }

    // タスク作成の 1 件を含む総数と一致し、同じ行を 2 回返していない
    let expected_total = seeded + 1;
    assert_eq!(
        seen.len(),
        expected_total,
        "ページを跨いで欠落・重複している: {} 件しか取れていない",
        seen.len()
    );
    let unique: std::collections::HashSet<&String> = seen.iter().collect();
    assert_eq!(unique.len(), expected_total, "同じ行が複数ページに出ている");
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
    seed_activities(&app, task_uuid, owner.id, seeded, Utc::now()).await;

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

    // cursor で続きが取れて、先頭のページと重ならない
    let cursor = next_cursor(&first_body).expect("続きがあるので next_cursor が返る");
    let second = app
        .get_with_session(&format!("{activities_path}?cursor={cursor}"))
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

    // 末尾は残り件数だけ返り、そこで next_cursor が消える
    let head = app
        .get_with_session(&format!("{activities_path}?limit=100"))
        .await;
    let head_body = json_body(head).await;
    let head_cursor = next_cursor(&head_body).expect("まだ残っている");
    let tail = app
        .get_with_session(&format!("{activities_path}?limit=100&cursor={head_cursor}"))
        .await;
    assert_eq!(tail.status(), StatusCode::OK);
    let tail_body = json_body(tail).await;
    assert_eq!(ids(&tail_body).len(), (total - 100) as usize);
    assert_eq!(
        next_cursor(&tail_body),
        None,
        "取り切ったら next_cursor は返さない"
    );
    assert_eq!(tail_body["total"].as_u64().expect("total"), total);

    // 壊れたカーソルは 400。利用者が作れる値なので 500 にしない
    let broken = app
        .get_with_session(&format!("{activities_path}?cursor=not-a-cursor"))
        .await;
    assert_eq!(broken.status(), StatusCode::BAD_REQUEST);
}

/// 読んでいる最中に履歴が積まれても、続きのページが重複も欠落もしない。
///
/// offset で継いでいたときは、1 ページ目を読んだ後に履歴が n 件積まれると
/// 2 ページ目の境界が n 件ぶん後ろへずれ、境界の行が二重に出ていた。
/// カーソルは並び順のキーそのものを持つので、前に積まれても位置が動かない。
#[tokio::test]
async fn activities_paging_is_not_shifted_by_rows_added_while_reading() {
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
            serde_json::json!({ "title": "読んでいる最中に増える", "status_id": status_id }),
        )
        .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let task_id = json_body(created).await["id"]
        .as_str()
        .expect("task id")
        .to_string();
    let task_uuid = Uuid::parse_str(&task_id).expect("uuid");

    // 1 ページ（20 件）では収まらない件数を積む。タスク作成の 1 件と合わせて 46 件
    let seeded = 45;
    let seed_base = Utc::now();
    seed_activities(&app, task_uuid, owner.id, seeded, seed_base).await;

    let activities_path = format!("{tasks_path}/{task_id}/activities");
    let first = app
        .get_with_session(&format!("{activities_path}?limit=20"))
        .await;
    assert_eq!(first.status(), StatusCode::OK);
    let first_body = json_body(first).await;
    let first_ids = ids(&first_body);
    assert_eq!(first_ids.len(), 20);
    let cursor = next_cursor(&first_body).expect("まだ残っている");

    // 1 ページ目を読んだ後に、別の操作で新しい履歴が積まれる。
    // 並びは新しい順なので、これは読み終えた側（先頭）に入る。
    // ページサイズ（20）より多く積んで、ずれが 1 ページぶんを越えても崩れないことを見る
    let added = 23;
    // 追加分は最初のページより確実に新しい側へ置く。helper 内で毎回 now() を
    // 取り直すと、初回 seed の未来方向の行より古い追加行が混ざってしまう。
    seed_activities(
        &app,
        task_uuid,
        owner.id,
        added,
        seed_base + Duration::seconds(seeded as i64 + 1),
    )
    .await;

    // カーソルで残りを読み切る
    let mut seen = first_ids.clone();
    let mut cursor = Some(cursor);
    while let Some(c) = cursor {
        let page = app
            .get_with_session(&format!("{activities_path}?limit=20&cursor={c}"))
            .await;
        assert_eq!(page.status(), StatusCode::OK);
        let body = json_body(page).await;
        seen.extend(ids(&body));
        cursor = next_cursor(&body);
    }

    let unique: std::collections::HashSet<&String> = seen.iter().collect();
    assert_eq!(
        unique.len(),
        seen.len(),
        "後から積まれた分だけ境界がずれて同じ行が 2 度出ている"
    );
    // 1 ページ目より古い行は 1 件も飛ばずに出る。後から積まれた分は
    // 読み終えた側に入るので、この読み方では出てこない
    assert_eq!(
        seen.len(),
        seeded + 1,
        "読み始めた時点の履歴を取りこぼしている"
    );
}
