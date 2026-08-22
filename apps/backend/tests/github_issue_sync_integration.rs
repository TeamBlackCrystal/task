mod common;

use axum::http::StatusCode;
use backend::utils::github::sync::{
    apply_issue_event, import_project, mark_pending_push, push_task,
};
use common::{TestApp, TestTenantProject};
use entity::{github_integrations, github_issue_links, project_statuses, tasks};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseBackend, EntityTrait,
    QueryFilter, QueryOrder, Statement,
};
use uuid::Uuid;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

const REPO_OWNER: &str = "acme";
const REPO_NAME: &str = "backend";

fn import_path(tp: &TestTenantProject) -> String {
    format!(
        "/v1/tenants/{}/projects/{}/github/import",
        tp.tenant_id, tp.project_id
    )
}

/// 積まれている取り込みジョブの件数。ペイロードは JSON なので project_id で絞る
/// （apalis.jobs.job は bytea。SeaORM の生 SQL では `?` ではなく `$N` を使う）。
async fn queued_import_jobs(app: &TestApp, project_id: Uuid) -> i64 {
    let row = app
        .state
        .db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT COUNT(*) AS count FROM apalis.jobs \
             WHERE job_type = $1 AND convert_from(job, 'UTF8') LIKE $2",
            [
                backend::jobs::github_issue_sync::QUEUE_NAME.into(),
                format!("%{project_id}%").into(),
            ],
        ))
        .await
        .expect("count import jobs")
        .expect("count row");
    row.try_get::<i64>("", "count").expect("count column")
}

fn unique_installation_id() -> i64 {
    400_000_000_000_i64 + (Uuid::new_v4().as_u128() % 900_000_000_000) as i64
}

fn issue_json(
    number: i32,
    title: &str,
    body: Option<&str>,
    state: &str,
    updated_at: &str,
) -> serde_json::Value {
    serde_json::json!({
        "number": number,
        "title": title,
        "body": body,
        "state": state,
        "updated_at": updated_at,
    })
}

/// インポート時点の Issue 更新時刻。イベントはこれより新しい時刻を使う。
const T_IMPORT: &str = "2026-08-01T00:00:00Z";
const T_EVENT: &str = "2026-08-02T00:00:00Z";
/// 書き戻し（PATCH）で GitHub 側の updated_at が進む先。T_EVENT より後。
const T_PUSH: &str = "2026-08-03T00:00:00Z";
/// 書き戻した後に、書き戻したのと同じ内容のイベントが届く時刻。T_PUSH より後にして、
/// 時刻のガードではなくハッシュ一致でループが止まることを確かめる
/// （自分の PATCH の跳ね返りそのものは updated_at が T_PUSH ちょうどで届くため、
/// ハッシュ比較の前に時刻のガードで捨てられる）。
const T_ECHO: &str = "2026-08-04T00:00:00Z";
const T_STALE: &str = "2026-07-01T00:00:00Z";

/// インポート対象のリポジトリ Issue 一覧。呼ばれるたび同じ 2 件 + PR 1 件を返す。
fn issue_list() -> serde_json::Value {
    serde_json::json!([
        issue_json(1, "ログインできない", Some("再現手順"), "open", T_IMPORT),
        issue_json(2, "古いバグ", None, "closed", T_IMPORT),
        {
            "number": 3,
            "title": "feat: 何か",
            "body": null,
            "state": "open",
            "updated_at": T_IMPORT,
            "pull_request": { "url": "https://api.github.com/repos/acme/backend/pulls/3" }
        },
    ])
}

async fn mount_mocks(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path_regex(r"^/app/installations/\d+/access_tokens$"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "token": "ghs_test_installation_token",
            "expires_at": "2030-01-01T00:00:00Z"
        })))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path(format!("/repos/{REPO_OWNER}/{REPO_NAME}/issues")))
        .respond_with(ResponseTemplate::new(200).set_body_json(issue_list()))
        .mount(server)
        .await;

    // GitHub と同じく更新後の Issue を返す。書き戻し側が使うのは updated_at だけ
    // （リンク行のウォーターマークを PATCH 後の時刻へ進めるため）なので、他のフィールドは
    // 固定にしている。番号もタイトルも状態も、実際に PATCH した内容とは対応していない。
    // 書き戻し側が updated_at 以外を見るようになったら、ここも送信内容に合わせること。
    Mock::given(method("PATCH"))
        .and(path_regex(r"^/repos/acme/backend/issues/\d+$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(issue_json(
            1,
            "書き戻し後",
            None,
            "open",
            T_PUSH,
        )))
        .mount(server)
        .await;
}

/// PATCH（書き戻し）が今まで何回飛んだか。
async fn patch_count(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .expect("received requests")
        .iter()
        .filter(|r| r.method == wiremock::http::Method::PATCH)
        .count()
}

/// 既定ステータスと完了ステータスを 1 つずつ用意する（`(default_id, done_id)`）。
async fn seed_statuses(app: &TestApp, project_id: Uuid) -> (Uuid, Uuid) {
    let mut ids = Vec::new();
    for (name, position, is_default, is_done) in
        [("Todo", 0i16, true, false), ("Done", 1i16, false, true)]
    {
        let id = Uuid::new_v4();
        project_statuses::ActiveModel {
            id: Set(id),
            project_id: Set(project_id),
            name: Set(name.into()),
            color: Set("#888888".into()),
            position: Set(position),
            is_default: Set(is_default),
            is_done_state: Set(is_done),
            created_at: Set(chrono::Utc::now().into()),
        }
        .insert(&app.state.db)
        .await
        .expect("insert status");
        ids.push(id);
    }
    (ids[0], ids[1])
}

async fn insert_integration(
    app: &TestApp,
    project_id: Uuid,
    created_by: Uuid,
) -> github_integrations::Model {
    github_integrations::ActiveModel {
        id: Set(Uuid::new_v4()),
        project_id: Set(project_id),
        installation_id: Set(unique_installation_id()),
        repo_owner: Set(REPO_OWNER.into()),
        repo_name: Set(REPO_NAME.into()),
        // ジョブ側はトークンを毎回取り直すため、この列の中身は同期に使われない。
        access_token_enc: Set("unused".into()),
        token_expires_at: Set(chrono::Utc::now().into()),
        created_by: Set(created_by),
        created_at: Set(chrono::Utc::now().into()),
    }
    .insert(&app.state.db)
    .await
    .expect("insert integration")
}

async fn project_tasks(app: &TestApp, project_id: Uuid) -> Vec<tasks::Model> {
    tasks::Entity::find()
        .filter(tasks::Column::ProjectId.eq(project_id))
        .order_by_asc(tasks::Column::SeqId)
        .all(&app.state.db)
        .await
        .expect("query tasks")
}

async fn link_for_number(
    app: &TestApp,
    project_id: Uuid,
    number: i32,
) -> github_issue_links::Model {
    github_issue_links::Entity::find()
        .filter(github_issue_links::Column::ProjectId.eq(project_id))
        .filter(github_issue_links::Column::GithubNumber.eq(number))
        .one(&app.state.db)
        .await
        .expect("query link")
        .expect("link row")
}

// serial: GITHUB_API_BASE_URL を差し替えるため、他の GitHub テストと並列に走らせない。
#[serial_test::serial]
#[tokio::test]
async fn github_issue_sync_suite() {
    let mock_server = MockServer::start().await;
    // SAFETY: serial アトリビュートにより他テストとの並列実行を防いでいる。
    unsafe {
        std::env::set_var("GITHUB_API_BASE_URL", mock_server.uri());
    }
    mount_mocks(&mock_server).await;

    let mut app = TestApp::new_with_github().await;
    let github = app
        .state
        .settings
        .github_app
        .clone()
        .expect("github app settings");

    // 1. POST /import — 未連携なら 404、非オーナーは 403、オーナーは 202
    {
        let owner = app.insert_user(false, false).await;
        let tp = app.insert_tenant_project(owner.id).await;
        app.login_session(&owner.email, &owner.password).await;

        let not_connected = app
            .post_json_with_session(&import_path(&tp), serde_json::json!({}))
            .await;
        assert_eq!(
            not_connected.status(),
            StatusCode::NOT_FOUND,
            "未連携プロジェクトのインポートは 404"
        );

        insert_integration(&app, tp.project_id, owner.id).await;

        let accepted = app
            .post_json_with_session(&import_path(&tp), serde_json::json!({}))
            .await;
        assert_eq!(
            accepted.status(),
            StatusCode::ACCEPTED,
            "オーナーのインポートは 202（過剰拒否でないこと）"
        );

        assert_eq!(
            queued_import_jobs(&app, tp.project_id).await,
            1,
            "202 を返したら取り込みジョブが 1 件積まれている"
        );

        // 連打しても、取り込み中のあいだは積み直さない（GitHub API のレート制限と
        // ワーカー時間を無駄に食わせない）。API は 202 のまま
        let repeated = app
            .post_json_with_session(&import_path(&tp), serde_json::json!({}))
            .await;
        assert_eq!(
            repeated.status(),
            StatusCode::ACCEPTED,
            "連打しても 202（取り込みは開始済みなのでエラーにしない）"
        );
        assert_eq!(
            queued_import_jobs(&app, tp.project_id).await,
            1,
            "連打しても取り込みジョブは増えない"
        );

        // 取り込みが終われば（ジョブ側が枠を返せば）また積める
        backend::utils::github::release_import_slot(&app.state.redis_client, tp.project_id)
            .await
            .expect("release import slot");
        let after_release = app
            .post_json_with_session(&import_path(&tp), serde_json::json!({}))
            .await;
        assert_eq!(
            after_release.status(),
            StatusCode::ACCEPTED,
            "枠が返っていれば再実行できる"
        );
        assert_eq!(
            queued_import_jobs(&app, tp.project_id).await,
            2,
            "枠が返っていれば取り込みジョブが積まれる"
        );

        app.reset_session_client();
        let outsider = app.insert_user(false, false).await;
        app.login_session(&outsider.email, &outsider.password).await;
        let forbidden = app
            .post_json_with_session(&import_path(&tp), serde_json::json!({}))
            .await;
        assert_eq!(
            forbidden.status(),
            StatusCode::FORBIDDEN,
            "テナントオーナー以外のインポートは 403"
        );

        app.cleanup_user(outsider.id).await;
        app.cleanup_user(owner.id).await;
        app.reset_session_client();
    }

    // 2. 一括インポート — open/closed が対応するステータスに入り、PR は取り込まれない
    {
        let owner = app.insert_user(false, false).await;
        let tp = app.insert_tenant_project(owner.id).await;
        let (todo_id, done_id) = seed_statuses(&app, tp.project_id).await;
        insert_integration(&app, tp.project_id, owner.id).await;

        let imported = import_project(
            &app.state.db,
            &app.state.http_client,
            &github,
            tp.project_id,
        )
        .await
        .expect("import");
        assert_eq!(imported, 2, "PR は取り込まない");

        let rows = project_tasks(&app, tp.project_id).await;
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].title, "ログインできない");
        assert_eq!(rows[0].description.as_deref(), Some("再現手順"));
        assert_eq!(rows[0].status_id, todo_id);
        assert!(rows[0].completed_at.is_none());
        assert_eq!(rows[1].title, "古いバグ");
        assert_eq!(rows[1].description, None, "本文 null は NULL のまま");
        assert_eq!(rows[1].status_id, done_id, "closed は完了ステータス");
        assert!(rows[1].completed_at.is_some());

        // 3. 同じ内容で再インポートしてもタスクは増えず、更新もされない
        let before = rows[0].updated_at;
        let again = import_project(
            &app.state.db,
            &app.state.http_client,
            &github,
            tp.project_id,
        )
        .await
        .expect("re-import");
        assert_eq!(again, 2);
        let rows = project_tasks(&app, tp.project_id).await;
        assert_eq!(rows.len(), 2, "重複したタスクを作らない");
        assert_eq!(rows[0].updated_at, before, "内容が同じなら書き込まない");

        app.cleanup_user(owner.id).await;
    }

    // 4. webhook の issues イベント — edited は反映し、deleted は反映しない
    {
        let owner = app.insert_user(false, false).await;
        let tp = app.insert_tenant_project(owner.id).await;
        let (_, done_id) = seed_statuses(&app, tp.project_id).await;
        insert_integration(&app, tp.project_id, owner.id).await;
        import_project(
            &app.state.db,
            &app.state.http_client,
            &github,
            tp.project_id,
        )
        .await
        .expect("import");

        let applied = apply_issue_event(
            &app.state.db,
            tp.project_id,
            &serde_json::json!({
                "action": "closed",
                "issue": issue_json(1, "ログインできない", Some("再現手順"), "closed", T_EVENT),
            }),
        )
        .await
        .expect("apply closed event");
        assert!(applied);

        let rows = project_tasks(&app, tp.project_id).await;
        assert_eq!(rows[0].status_id, done_id, "closed イベントで完了になる");
        assert!(rows[0].completed_at.is_some());

        let applied = apply_issue_event(
            &app.state.db,
            tp.project_id,
            &serde_json::json!({
                "action": "deleted",
                "issue": issue_json(9, "消えた Issue", None, "open", T_EVENT),
            }),
        )
        .await
        .expect("apply deleted event");
        assert!(!applied, "deleted は取り込まない");
        assert_eq!(
            project_tasks(&app, tp.project_id).await.len(),
            2,
            "deleted で新しいタスクを作らない"
        );

        app.cleanup_user(owner.id).await;
    }

    // 5. 書き戻し — タスク側の変更を PATCH し、同じ内容の再送・跳ね返りでは書かない
    {
        let owner = app.insert_user(false, false).await;
        let tp = app.insert_tenant_project(owner.id).await;
        let (_, done_id) = seed_statuses(&app, tp.project_id).await;
        insert_integration(&app, tp.project_id, owner.id).await;
        import_project(
            &app.state.db,
            &app.state.http_client,
            &github,
            tp.project_id,
        )
        .await
        .expect("import");

        let task = project_tasks(&app, tp.project_id).await.remove(0);
        let before_patches = patch_count(&mock_server).await;

        // 変更が無ければ書き戻さない
        push_task(&app.state.db, &app.state.http_client, &github, task.id)
            .await
            .expect("push unchanged");
        assert_eq!(
            patch_count(&mock_server).await,
            before_patches,
            "内容が同じなら GitHub を叩かない"
        );

        // タスク側でタイトルと状態を変える → 1 回だけ PATCH
        let mut active: tasks::ActiveModel = task.clone().into();
        active.title = Set("ログインできない（調査中）".into());
        active.status_id = Set(done_id);
        active.update(&app.state.db).await.expect("update task");

        // 書き戻し要求はハンドラーがタスク更新と同一トランザクションで pending_push に永続化する
        let updated_at_before = link_for_number(&app, tp.project_id, 1).await.updated_at;
        let marked = mark_pending_push(&app.state.db, task.id)
            .await
            .expect("mark pending");
        assert!(marked, "リンク済みタスクは pending_push が立つ");
        let marked_link = link_for_number(&app, tp.project_id, 1).await;
        assert!(marked_link.pending_push);
        // push_task の条件付き更新は updated_at を版として見る。ここが進まないと
        // 「読み取った後に立った要求」を消してしまう。
        assert!(
            marked_link.updated_at > updated_at_before,
            "pending_push を立てたら updated_at も進む"
        );

        push_task(&app.state.db, &app.state.http_client, &github, task.id)
            .await
            .expect("push changed");
        assert_eq!(
            patch_count(&mock_server).await,
            before_patches + 1,
            "変更は 1 回書き戻す"
        );
        assert!(
            !link_for_number(&app, tp.project_id, 1).await.pending_push,
            "書き戻し後は pending_push が消える"
        );

        push_task(&app.state.db, &app.state.http_client, &github, task.id)
            .await
            .expect("push again");
        assert_eq!(
            patch_count(&mock_server).await,
            before_patches + 1,
            "同じ内容を二度書き戻さない"
        );

        // 書き戻した内容が webhook で返ってくる（GitHub → タスクのループ）
        let link = link_for_number(&app, tp.project_id, 1).await;
        let synced_before = link.synced_hash.clone();
        let updated_before = project_tasks(&app, tp.project_id)
            .await
            .remove(0)
            .updated_at;

        // 自分の PATCH の跳ね返りは updated_at が T_PUSH ちょうどで届く。ウォーターマークが
        // すでに T_PUSH なので、ハッシュ比較より前に時刻のガード（<=）で捨てられる。
        apply_issue_event(
            &app.state.db,
            tp.project_id,
            &serde_json::json!({
                "action": "edited",
                "issue": issue_json(1, "ログインできない（調査中）", Some("再現手順"), "closed", T_PUSH),
            }),
        )
        .await
        .expect("apply patch echo");
        assert_eq!(
            link_for_number(&app, tp.project_id, 1).await.updated_at,
            link.updated_at,
            "書き戻しと同じ時刻のイベントは何も書かずに捨てる"
        );

        let applied = apply_issue_event(
            &app.state.db,
            tp.project_id,
            &serde_json::json!({
                "action": "edited",
                "issue": issue_json(1, "ログインできない（調査中）", Some("再現手順"), "closed", T_ECHO),
            }),
        )
        .await
        .expect("apply echo event");
        assert!(applied, "イベントは処理される");
        assert_eq!(
            project_tasks(&app, tp.project_id)
                .await
                .remove(0)
                .updated_at,
            updated_before,
            "自分が書き戻した内容の跳ね返りではタスクを更新しない"
        );
        let after_echo = link_for_number(&app, tp.project_id, 1).await;
        assert_eq!(after_echo.synced_hash, synced_before);
        // 時刻のガードで捨てられたのではなく、ハッシュ一致の経路を通ったことを確かめる
        // （捨てられていれば T_PUSH のまま止まる）。
        assert_eq!(
            after_echo.github_updated_at.with_timezone(&chrono::Utc),
            T_ECHO
                .parse::<chrono::DateTime<chrono::Utc>>()
                .expect("parse T_ECHO"),
            "内容が同じイベントでも、より新しい時刻ならウォーターマークは進む"
        );
        // ここで版を進めないと、書き戻し中に同じ内容のイベントが入ったとき
        // push_task の条件付き更新が空振りせず、ウォーターマークを後退させる。
        assert!(
            after_echo.updated_at > link.updated_at,
            "ウォーターマークだけ進めるときも版を進める"
        );

        app.cleanup_user(owner.id).await;
    }

    // 6. 遅延・再送された古いイベントは新しい内容を巻き戻さない
    {
        let owner = app.insert_user(false, false).await;
        let tp = app.insert_tenant_project(owner.id).await;
        seed_statuses(&app, tp.project_id).await;
        insert_integration(&app, tp.project_id, owner.id).await;
        import_project(
            &app.state.db,
            &app.state.http_client,
            &github,
            tp.project_id,
        )
        .await
        .expect("import");

        // Issue が open のままタイトルだけ編集されても、タスク固有のワークフロー状態は保持する。
        let in_progress_id = Uuid::new_v4();
        project_statuses::ActiveModel {
            id: Set(in_progress_id),
            project_id: Set(tp.project_id),
            name: Set("In Progress".into()),
            color: Set("#888888".into()),
            position: Set(1),
            is_default: Set(false),
            is_done_state: Set(false),
            created_at: Set(chrono::Utc::now().into()),
        }
        .insert(&app.state.db)
        .await
        .expect("insert in-progress status");
        let task = project_tasks(&app, tp.project_id).await.remove(0);
        let mut task_active: tasks::ActiveModel = task.into();
        task_active.status_id = Set(in_progress_id);
        task_active
            .update(&app.state.db)
            .await
            .expect("set in-progress");

        // 新しいイベントでタイトルが変わる
        apply_issue_event(
            &app.state.db,
            tp.project_id,
            &serde_json::json!({
                "action": "edited",
                "issue": issue_json(1, "新しいタイトル", Some("再現手順"), "open", T_EVENT),
            }),
        )
        .await
        .expect("apply new event");
        assert_eq!(
            project_tasks(&app, tp.project_id).await[0].title,
            "新しいタイトル"
        );
        assert_eq!(
            project_tasks(&app, tp.project_id).await[0].status_id,
            in_progress_id,
            "open Issue の編集ではタスクのワークフロー状態を保持する"
        );

        // その後に届いた古いイベント（インポート時点より前の内容）は捨てられる
        apply_issue_event(
            &app.state.db,
            tp.project_id,
            &serde_json::json!({
                "action": "edited",
                "issue": issue_json(1, "古いタイトル", Some("昔の本文"), "open", T_STALE),
            }),
        )
        .await
        .expect("apply stale event");
        assert_eq!(
            project_tasks(&app, tp.project_id).await[0].title,
            "新しいタイトル",
            "古いイベントで巻き戻らない"
        );

        app.cleanup_user(owner.id).await;
    }

    // 7. 連携解除（integration 削除）でリンクがカスケード削除され、別リポジトリへの再連携後に
    //    残った書き戻しジョブが走っても Issue を触らない
    {
        let owner = app.insert_user(false, false).await;
        let tp = app.insert_tenant_project(owner.id).await;
        seed_statuses(&app, tp.project_id).await;
        let integration = insert_integration(&app, tp.project_id, owner.id).await;
        import_project(
            &app.state.db,
            &app.state.http_client,
            &github,
            tp.project_id,
        )
        .await
        .expect("import");
        let task = project_tasks(&app, tp.project_id).await.remove(0);

        github_integrations::Entity::delete_by_id(integration.id)
            .exec(&app.state.db)
            .await
            .expect("delete integration");

        let remaining = github_issue_links::Entity::find()
            .filter(github_issue_links::Column::ProjectId.eq(tp.project_id))
            .all(&app.state.db)
            .await
            .expect("query links");
        assert!(remaining.is_empty(), "連携解除でリンクも消える");

        // リンクが消えているので、残っていた書き戻しジョブが走っても GitHub を叩かない
        let before_patches = patch_count(&mock_server).await;
        push_task(&app.state.db, &app.state.http_client, &github, task.id)
            .await
            .expect("push after unlink");
        assert_eq!(
            patch_count(&mock_server).await,
            before_patches,
            "解除後の書き戻しは no-op"
        );

        app.cleanup_user(owner.id).await;
    }

    // 8. 書き戻しより前に GitHub 側で起きた編集が、書き戻しの後に届いても巻き戻さない
    {
        let owner = app.insert_user(false, false).await;
        let tp = app.insert_tenant_project(owner.id).await;
        seed_statuses(&app, tp.project_id).await;
        insert_integration(&app, tp.project_id, owner.id).await;
        import_project(
            &app.state.db,
            &app.state.http_client,
            &github,
            tp.project_id,
        )
        .await
        .expect("import");

        // ローカルの変更を書き戻す。GitHub 側の updated_at は PATCH で T_PUSH まで進む
        let task = project_tasks(&app, tp.project_id).await.remove(0);
        let mut active: tasks::ActiveModel = task.clone().into();
        active.title = Set("ローカルで直した".into());
        active.update(&app.state.db).await.expect("update task");
        push_task(&app.state.db, &app.state.http_client, &github, task.id)
            .await
            .expect("push");

        // 書き戻しより前（T_EVENT）に GitHub 側で起きていた編集の webhook が、遅れて届く
        apply_issue_event(
            &app.state.db,
            tp.project_id,
            &serde_json::json!({
                "action": "edited",
                "issue": issue_json(1, "GitHub 側の古い編集", Some("昔の本文"), "open", T_EVENT),
            }),
        )
        .await
        .expect("apply delayed event");

        assert_eq!(
            project_tasks(&app, tp.project_id).await[0].title,
            "ローカルで直した",
            "書き戻し前の編集が遅れて届いてもローカルの変更を巻き戻さない"
        );
        assert_eq!(
            link_for_number(&app, tp.project_id, 1)
                .await
                .github_updated_at
                .with_timezone(&chrono::Utc),
            T_PUSH
                .parse::<chrono::DateTime<chrono::Utc>>()
                .expect("parse T_PUSH"),
            "遅れて届いた古いイベントでウォーターマークを巻き戻さない"
        );

        app.cleanup_user(owner.id).await;
    }
}
