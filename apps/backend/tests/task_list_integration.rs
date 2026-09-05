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

fn task_ids(body: &serde_json::Value) -> Vec<String> {
    body["tasks"]
        .as_array()
        .expect("tasks array")
        .iter()
        .map(|t| t["id"].as_str().expect("id").to_string())
        .collect()
}

/// 次のページの鍵。取り切っていれば `None`。
fn next_cursor(body: &serde_json::Value) -> Option<String> {
    body["next_cursor"].as_str().map(|s| s.to_string())
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
/// List 表示はステータスごとに `cursor` でページを継ぐ。既定の並びは `created_at DESC`
/// だけだったので、同じ `created_at` を持つタスクが境界に来ると、静的なデータへの
/// 連続リクエストでも同じタスクが複数ページに出たり、別のタスクがどのページにも
/// 出なかったりする。frontend の `toTaskGroup()` は重複 ID を落とすので重複は隠れるが、
/// 欠落は戻らない。カーソルの不等式も `ORDER BY` と同じ形でないと同じことが起きる。
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
    let mut cursor: Option<String> = None;
    loop {
        let url = match &cursor {
            Some(c) => format!("{base}?limit=20&cursor={c}"),
            None => format!("{base}?limit=20"),
        };
        let page = app.get_with_session(&url).await;
        assert_eq!(page.status(), StatusCode::OK);
        let body: serde_json::Value = page.json().await.expect("list json");
        seen.extend(task_ids(&body));
        match next_cursor(&body) {
            Some(c) => cursor = Some(c),
            None => break,
        }
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
    let mut by_id_asc = created.clone();
    by_id_asc.sort();
    assert_ne!(by_id_asc, created, "作成順と ID 順が偶然一致した");
    // 一覧のタイブレーカーは id DESC。カーソルの不等式（TaskCursor::keyset_after の
    // `id.lt(...)`）がその向きに合わせてあるので、揃えないとページ境界で行が飛ぶ
    let by_id_desc: Vec<String> = by_id_asc.iter().rev().cloned().collect();

    for sort in ["priority_asc", "deadline_asc"] {
        let mut read = Vec::new();
        for offset in [0, 3, 6] {
            read.extend(list_page_ids(&app, &tp, sort, 3, offset).await);
        }
        assert_eq!(read, by_id_desc, "sort={sort}");
    }

    // 検索側はカーソルを持たないので、タイブレーカーは id ASC のまま
    let mut read = Vec::new();
    for offset in [0, 3, 6] {
        read.extend(search_page_ids(&app, &tp, "paging", 3, offset).await);
    }
    assert_eq!(read, by_id_asc, "search");

    app.cleanup_user(user.id).await;
}

/// 読んでいる最中にタスクが一覧から外れても、続きのページが 1 件も飛ばさない。
///
/// これが offset ページングで欠落が出る筋。1 ページ目を読んだ後に、そのページより
/// 前（新しい側）のタスクが別のステータスへ移ると、後続のタスクが 1 件ぶん前へ詰まる。
/// `offset=20` は詰まった後の 21 件目を指すので、境界にいたタスクがどのページにも
/// 出てこない。しかも最後のページが埋まらずに終わるため、「もっと見る」も消える。
/// カーソルは並び順のキーそのものを持つので、前が減っても位置が動かない。
#[tokio::test]
async fn task_list_paging_keeps_rows_that_shift_when_earlier_ones_leave_the_filter() {
    let mut app = TestApp::new().await;
    let (_user, tp) = setup_project(&mut app).await;
    let todo_id = create_status(&app, &tp).await;
    let base = tasks_base(&tp);

    // 「移動先」のステータス。1 ページ目のタスクをここへ逃がして一覧から外す
    let statuses_path = format!(
        "/v1/tenants/{}/projects/{}/statuses",
        tp.tenant_id, tp.project_id
    );
    let done = app
        .post_json_with_session(
            &statuses_path,
            serde_json::json!({
                "name": "Done",
                "color": "#22aa66",
                "position": 2,
                "is_default": false,
                "is_done_state": true,
            }),
        )
        .await;
    assert_eq!(done.status(), StatusCode::CREATED);
    let done_body: serde_json::Value = done.json().await.expect("status json");
    let done_id = done_body["id"].as_str().expect("status id").to_string();

    // ページサイズ（20）を越える件数。境界の前後に十分な行を置く
    let count = 45;
    let mut created = Vec::new();
    for i in 0..count {
        let response = app
            .post_json_with_session(
                &base,
                serde_json::json!({ "title": format!("Todo {i}"), "status_id": todo_id }),
            )
            .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let body: serde_json::Value = response.json().await.expect("task json");
        created.push(body["id"].as_str().expect("id").to_string());
    }

    let scoped = format!("{base}?status_id={todo_id}&limit=20");
    let first = app.get_with_session(&scoped).await;
    assert_eq!(first.status(), StatusCode::OK);
    let first_body: serde_json::Value = first.json().await.expect("list json");
    let first_ids = task_ids(&first_body);
    assert_eq!(first_ids.len(), 20);
    let cursor = next_cursor(&first_body).expect("まだ残っている");

    // 1 ページ目にいたタスクを 3 件、別のステータスへ移す（Todo の一覧から外れる）。
    // offset ならここで後続が 3 件ぶん前へ詰まり、境界の 3 件が飛ぶ
    let moved = 3;
    for id in first_ids.iter().take(moved) {
        let response = app
            .put_json_with_session(
                &format!("{base}/{id}"),
                serde_json::json!({ "status_id": done_id }),
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK, "move task out of Todo");
    }

    // カーソルで残りを読み切る
    let mut seen = first_ids.clone();
    let mut cursor = Some(cursor);
    while let Some(c) = cursor {
        let page = app.get_with_session(&format!("{scoped}&cursor={c}")).await;
        assert_eq!(page.status(), StatusCode::OK);
        let body: serde_json::Value = page.json().await.expect("list json");
        seen.extend(task_ids(&body));
        cursor = next_cursor(&body);
    }

    let unique: std::collections::HashSet<&String> = seen.iter().collect();
    assert_eq!(unique.len(), seen.len(), "同じタスクが複数ページに出ている");
    assert_eq!(
        seen.len(),
        count,
        "読んでいる最中に前のタスクが外れたぶんだけ後続が飛んでいる"
    );
    // 移した 3 件も含め、作った全件がどこかのページに出ている
    let seen_set: std::collections::HashSet<&String> = seen.iter().collect();
    for id in &created {
        assert!(
            seen_set.contains(id),
            "タスク {id} がどのページにも出ていない"
        );
    }
}

/// カーソルは並びごとに別物なので、`sort` を変えたまま使い回せない。
///
/// 黙って先頭へ戻すと、並び替えのたびに一覧が巻き戻って原因が見えなくなる。
#[tokio::test]
async fn task_list_rejects_a_cursor_made_for_another_sort() {
    let mut app = TestApp::new().await;
    let (_user, tp) = setup_project(&mut app).await;
    let status_id = create_status(&app, &tp).await;
    let base = tasks_base(&tp);

    for i in 0..3 {
        let response = app
            .post_json_with_session(
                &base,
                serde_json::json!({ "title": format!("並び替え {i}"), "status_id": status_id }),
            )
            .await;
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    // 既定（created_at_desc）の 1 ページ目からカーソルを取る
    let first = app.get_with_session(&format!("{base}?limit=1")).await;
    assert_eq!(first.status(), StatusCode::OK);
    let first_body: serde_json::Value = first.json().await.expect("list json");
    let cursor = next_cursor(&first_body).expect("まだ残っている");

    // 同じ並びなら続きが取れる（過剰に拒否していないこと）
    let same = app
        .get_with_session(&format!("{base}?limit=1&cursor={cursor}"))
        .await;
    assert_eq!(same.status(), StatusCode::OK);
    assert_eq!(task_ids(&same.json().await.expect("list json")).len(), 1);

    // 別の並びへ持ち込むと 400
    let crossed = app
        .get_with_session(&format!("{base}?limit=1&sort=priority_asc&cursor={cursor}"))
        .await;
    assert_eq!(crossed.status(), StatusCode::BAD_REQUEST);

    // offset と併用も 400（起点が二重になる）
    let mixed = app
        .get_with_session(&format!("{base}?limit=1&offset=20&cursor={cursor}"))
        .await;
    assert_eq!(mixed.status(), StatusCode::BAD_REQUEST);
}
