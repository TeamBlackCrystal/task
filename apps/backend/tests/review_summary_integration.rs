mod common;

use axum::http::StatusCode;
use common::TestApp;
use entity::{github_integrations, project_statuses};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ConnectionTrait, prelude::Uuid};
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

// レビュー要約コメント（仕様 §7）の統合テスト。
//
// 「1 PR に 1 本だけ置き、2 回目以降は同じコメントを編集する」ことと、
// 連携の無いプロジェクトでは何もしないことを、モックサーバー相手に固定する。

const REPO_OWNER: &str = "acme";
const REPO_NAME: &str = "backend";
const PR_NUMBER: i32 = 618;
const COMMENT_ID: i64 = 4242;
/// 鍵の単位に使うリポジトリ（連携先を差し替えると別の鍵になる）
const REPO_KEY: &str = "acme/backend";
/// テストの GitHub App 名（`GITHUB_APP_NAME`）から作られる bot の login
const BOT_LOGIN: &str = "task-app[bot]";
/// ラウンドが見た commit。PR メタの head と揃えると「鮮度あり」になる
const REVIEWED_HEAD: &str = "60cdd7795f94fa4e4148ce996c2efb4c363e3f5e";

fn unique_installation_id() -> i64 {
    // 同じ DB を共有する他テストと衝突しない範囲で散らす
    (Uuid::new_v4().as_u128() % 1_000_000) as i64 + 8_000_000
}

async fn mount_mocks(server: &MockServer, existing_comment: bool, marker: &str) {
    Mock::given(method("POST"))
        .and(path_regex(r"^/app/installations/\d+/access_tokens$"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "token": "ghs_test_installation_token",
            "expires_at": "2030-01-01T00:00:00Z"
        })))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path(format!(
            "/repos/{REPO_OWNER}/{REPO_NAME}/pulls/{PR_NUMBER}"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "title": "feat: レビュー指摘管理",
            "user": { "login": "yupix" },
            "head": { "sha": REVIEWED_HEAD }
        })))
        .mount(server)
        .await;

    // 既存コメントの有無で「新規投稿」と「編集」を出し分ける
    let comments = if existing_comment {
        serde_json::json!([
            { "id": 1, "body": "無関係なコメント" },
            {
                "id": COMMENT_ID,
                "body": format!("{}\n古い要約", marker),
                "user": { "login": BOT_LOGIN, "type": "Bot" }
            },
        ])
    } else {
        serde_json::json!([{ "id": 1, "body": "無関係なコメント" }])
    };
    Mock::given(method("GET"))
        .and(path(format!(
            "/repos/{REPO_OWNER}/{REPO_NAME}/issues/{PR_NUMBER}/comments"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(comments))
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path(format!(
            "/repos/{REPO_OWNER}/{REPO_NAME}/issues/{PR_NUMBER}/comments"
        )))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(serde_json::json!({ "id": COMMENT_ID })),
        )
        .mount(server)
        .await;

    Mock::given(method("PATCH"))
        .and(path(format!(
            "/repos/{REPO_OWNER}/{REPO_NAME}/issues/comments/{COMMENT_ID}"
        )))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": COMMENT_ID })),
        )
        .mount(server)
        .await;
}

/// 指定メソッドで送られたリクエストの本文を集める。
async fn bodies_of(server: &MockServer, wanted: wiremock::http::Method) -> Vec<serde_json::Value> {
    server
        .received_requests()
        .await
        .expect("received requests")
        .iter()
        .filter(|r| r.method == wanted)
        .filter_map(|r| serde_json::from_slice::<serde_json::Value>(&r.body).ok())
        .collect()
}

async fn seed_statuses(app: &TestApp, project_id: Uuid) {
    for (name, position, is_default, is_done) in
        [("Todo", 0i16, true, false), ("Done", 1i16, false, true)]
    {
        project_statuses::ActiveModel {
            id: Set(Uuid::new_v4()),
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
    }
}

async fn link_integration(app: &TestApp, project_id: Uuid, created_by: Uuid) {
    github_integrations::ActiveModel {
        id: Set(Uuid::new_v4()),
        project_id: Set(project_id),
        installation_id: Set(unique_installation_id()),
        repo_owner: Set(REPO_OWNER.into()),
        repo_name: Set(REPO_NAME.into()),
        // ジョブ側はトークンを毎回取り直すため、この列の中身は使われない
        access_token_enc: Set("unused".into()),
        token_expires_at: Set(chrono::Utc::now().into()),
        created_by: Set(created_by),
        created_at: Set(chrono::Utc::now().into()),
    }
    .insert(&app.state.db)
    .await
    .expect("insert integration");
}

/// 積まれている要約更新ジョブの件数。ペイロードは JSON なので project_id で絞る
/// （apalis.jobs.job は bytea。SeaORM の生 SQL では `?` ではなく `$N` を使う）。
async fn queued_summary_jobs(app: &TestApp, project_id: Uuid) -> i64 {
    let row = app
        .state
        .db
        .query_one_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT COUNT(*) AS count FROM apalis.jobs \
             WHERE job_type = $1 AND convert_from(job, 'UTF8') LIKE $2",
            [
                job::review_summary::QUEUE_NAME.into(),
                format!("%{project_id}%").into(),
            ],
        ))
        .await
        .expect("count review summary jobs")
        .expect("count row");
    row.try_get::<i64>("", "count").expect("count column")
}

fn job_state(app: &TestApp) -> job::JobState {
    job::JobState {
        settings: app.state.settings.clone(),
        db: app.state.db.clone(),
        redis_client: app.state.redis_client.clone(),
        smtp_client: app.state.smtp_client.clone(),
        http_client: app.state.http_client.clone(),
        review_summary_storage: app.state.review_summary_storage.clone(),
    }
}

// serial: GITHUB_API_BASE_URL を差し替えるため、他の GitHub テストと並列に走らせない。
#[serial_test::serial]
#[tokio::test]
async fn review_summary_comment_is_created_once_and_then_edited() {
    let mock_server = MockServer::start().await;
    // SAFETY: serial アトリビュートにより他テストとの並列実行を防いでいる。
    unsafe {
        std::env::set_var("GITHUB_API_BASE_URL", mock_server.uri());
    }
    let mut app = TestApp::new_with_github().await;
    let reviewer = app.insert_user_default().await;
    let tp = app.insert_tenant_project(reviewer.id).await;
    seed_statuses(&app, tp.project_id).await;
    link_integration(&app, tp.project_id, reviewer.id).await;
    let marker = service::github::pr_comments::summary_marker(tp.project_id);
    mount_mocks(&mock_server, false, &marker).await;

    app.reset_session_client();
    app.login_session_no_content(&reviewer.email, &reviewer.password)
        .await;

    let reviews_path = format!(
        "/v1/tenants/{}/projects/{}/reviews",
        tp.tenant_id, tp.project_id
    );
    let res = app
        .post_json_with_session(
            &reviews_path,
            serde_json::json!({
                "pr_number": PR_NUMBER,
                "head_sha": "60cdd7795f94fa4e4148ce996c2efb4c363e3f5e",
                "summary": "総評",
                "findings": [
                    { "severity": "high", "title": "認可漏れ", "body": "本文" },
                    { "severity": "low", "title": "命名", "body": "本文" },
                ],
            }),
        )
        .await;
    assert_eq!(res.status(), StatusCode::CREATED);

    // ワーカーはテストハーネスで動かさないので、ジョブ本体を直接呼ぶ
    let state = job_state(&app);
    job::review_summary::process(
        job::ReviewSummaryJob {
            project_id: tp.project_id,
            pr_number: PR_NUMBER,
        },
        apalis::prelude::Data::new(state.clone()),
    )
    .await
    .expect("post review summary");

    // 既存コメントが無いので新規投稿。マーカーとマージ可否が本文に出る
    let posted = bodies_of(&mock_server, wiremock::http::Method::POST).await;
    let posted: Vec<&serde_json::Value> =
        posted.iter().filter(|b| b.get("body").is_some()).collect();
    assert_eq!(posted.len(), 1, "要約コメントは 1 本だけ作る");
    let body = posted[0]["body"].as_str().expect("comment body");
    assert!(body.starts_with(&marker), "マーカーが行頭にある: {body}");
    assert!(body.contains("マージ不可"), "High が未解決: {body}");
    assert!(body.contains("| High | Open | 1 |"), "件数表が出る: {body}");

    // PR メタ（タイトル・作者）がラウンドにキャッシュされる
    let rounds = app
        .get_with_session(&format!("{reviews_path}?pr={PR_NUMBER}"))
        .await
        .json::<serde_json::Value>()
        .await
        .expect("rounds json");
    assert_eq!(rounds[0]["pr_title"], "feat: レビュー指摘管理");
    assert_eq!(rounds[0]["pr_author"], "yupix");

    app.cleanup_user(reviewer.id).await;
}

/// 2 回目以降は POST せず、同じコメントを PATCH で更新する。
#[serial_test::serial]
#[tokio::test]
async fn an_existing_summary_comment_is_updated_in_place() {
    let mock_server = MockServer::start().await;
    // SAFETY: serial アトリビュートにより他テストとの並列実行を防いでいる。
    unsafe {
        std::env::set_var("GITHUB_API_BASE_URL", mock_server.uri());
    }
    let mut app = TestApp::new_with_github().await;
    let reviewer = app.insert_user_default().await;
    let tp = app.insert_tenant_project(reviewer.id).await;
    seed_statuses(&app, tp.project_id).await;
    link_integration(&app, tp.project_id, reviewer.id).await;
    let marker = service::github::pr_comments::summary_marker(tp.project_id);
    mount_mocks(&mock_server, true, &marker).await;

    app.reset_session_client();
    app.login_session_no_content(&reviewer.email, &reviewer.password)
        .await;

    let res = app
        .post_json_with_session(
            &format!(
                "/v1/tenants/{}/projects/{}/reviews",
                tp.tenant_id, tp.project_id
            ),
            serde_json::json!({
                "pr_number": PR_NUMBER,
                "head_sha": REVIEWED_HEAD,
                "summary": "指摘なし",
                "findings": [],
            }),
        )
        .await;
    assert_eq!(res.status(), StatusCode::CREATED);

    job::review_summary::process(
        job::ReviewSummaryJob {
            project_id: tp.project_id,
            pr_number: PR_NUMBER,
        },
        apalis::prelude::Data::new(job_state(&app)),
    )
    .await
    .expect("update review summary");

    let posted = bodies_of(&mock_server, wiremock::http::Method::POST).await;
    assert!(
        posted.iter().all(|b| b.get("body").is_none()),
        "既存コメントがあれば新規投稿しない（PR にコメントを積み上げない）"
    );

    let patched = bodies_of(&mock_server, wiremock::http::Method::PATCH).await;
    assert_eq!(patched.len(), 1, "同じコメントを 1 回だけ編集する");
    let body = patched[0]["body"].as_str().expect("comment body");
    assert!(body.starts_with(&marker));
    assert!(body.contains("マージ可"), "指摘なしならマージ可: {body}");
    assert!(body.contains("指摘はありません。"));

    app.cleanup_user(reviewer.id).await;
}

/// 連続した状態遷移でも更新ジョブは 1 本に合流し、コメントは 1 本のまま最新になる。
///
/// 遷移のたびにジョブを積むと、同じコメントへ短時間に何度も書き込みに行き、
/// GitHub の secondary rate limit に当たる（仕様 §7）。
#[serial_test::serial]
#[tokio::test]
async fn consecutive_transitions_coalesce_into_one_summary_update() {
    let mock_server = MockServer::start().await;
    // SAFETY: serial アトリビュートにより他テストとの並列実行を防いでいる。
    unsafe {
        std::env::set_var("GITHUB_API_BASE_URL", mock_server.uri());
    }
    let mut app = TestApp::new_with_github().await;
    let reviewer = app.insert_user_default().await;
    let tp = app.insert_tenant_project(reviewer.id).await;
    seed_statuses(&app, tp.project_id).await;
    link_integration(&app, tp.project_id, reviewer.id).await;
    let marker = service::github::pr_comments::summary_marker(tp.project_id);
    mount_mocks(&mock_server, false, &marker).await;

    app.reset_session_client();
    app.login_session_no_content(&reviewer.email, &reviewer.password)
        .await;

    let reviews_path = format!(
        "/v1/tenants/{}/projects/{}/reviews",
        tp.tenant_id, tp.project_id
    );
    let res = app
        .post_json_with_session(
            &reviews_path,
            serde_json::json!({
                "pr_number": PR_NUMBER,
                "head_sha": "60cdd7795f94fa4e4148ce996c2efb4c363e3f5e",
                "summary": "総評",
                "findings": [
                    { "severity": "high", "title": "認可漏れ", "body": "本文" },
                    { "severity": "high", "title": "検証漏れ", "body": "本文" },
                    { "severity": "medium", "title": "境界値", "body": "本文" },
                ],
            }),
        )
        .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let finding_ids: Vec<String> = res.json::<serde_json::Value>().await.expect("json")["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .map(|f| f["id"].as_str().expect("finding id").to_string())
        .collect();

    // 起票で 1 本積まれている。ここから 3 件を続けて fixed にしても増えない
    let findings_path = format!(
        "/v1/tenants/{}/projects/{}/review-findings",
        tp.tenant_id, tp.project_id
    );
    for finding_id in &finding_ids {
        let res = app
            .patch_json_with_session(
                &format!("{findings_path}/{finding_id}"),
                serde_json::json!({ "state": "fixed" }),
            )
            .await;
        assert_eq!(res.status(), StatusCode::OK);
    }

    assert!(
        !service::github::review_summary_queue::try_mark_pending(
            &app.state.redis_client,
            tp.project_id,
            REPO_KEY,
            PR_NUMBER
        )
        .await
        .expect("pending flag"),
        "起票と 3 件の遷移は 1 本の更新に合流する"
    );

    // 積まれている 1 本を走らせると、最新状態（3 件とも fixed）で 1 回だけ投稿する
    let state = job_state(&app);
    job::review_summary::process(
        job::ReviewSummaryJob {
            project_id: tp.project_id,
            pr_number: PR_NUMBER,
        },
        apalis::prelude::Data::new(state.clone()),
    )
    .await
    .expect("post review summary");

    let posted = bodies_of(&mock_server, wiremock::http::Method::POST).await;
    let posted: Vec<&serde_json::Value> =
        posted.iter().filter(|b| b.get("body").is_some()).collect();
    assert_eq!(posted.len(), 1, "書き込みは 1 回だけ");
    let body = posted[0]["body"].as_str().expect("comment body");
    assert!(
        body.contains("| High | Fixed | 2 |"),
        "最新の件数が出る: {body}"
    );
    assert!(
        body.contains("| Medium | Fixed | 1 |"),
        "最新の件数が出る: {body}"
    );
    assert!(
        body.contains("マージ不可"),
        "fixed は未解決として数える: {body}"
    );

    // ジョブが走ったので、次の遷移はまた積める
    assert!(
        service::github::review_summary_queue::try_mark_pending(
            &app.state.redis_client,
            tp.project_id,
            REPO_KEY,
            PR_NUMBER
        )
        .await
        .expect("pending flag"),
        "ジョブの実行後は次の更新を受け付ける"
    );

    app.cleanup_user(reviewer.id).await;
}

/// 担い手が消えても詰まらないよう、印とロックは TTL 付きで、
/// ロックの解放は取得時のトークンと一致するときだけ効く（仕様 §7）。
#[tokio::test]
async fn the_pending_flag_and_lock_expire_and_release_is_owner_checked() {
    use service::github::review_summary_queue as queue;

    let app = TestApp::new().await;
    let redis = &app.state.redis_client;
    // Redis だけを使うので、実在するプロジェクトでなくてよい
    let project_id = Uuid::new_v4();
    let pr = 618;

    assert!(
        queue::try_mark_pending(redis, project_id, REPO_KEY, pr)
            .await
            .expect("mark pending")
    );
    let token = queue::try_acquire_update_lock(redis, project_id, REPO_KEY, pr)
        .await
        .expect("acquire lock")
        .expect("空いていれば取れる");

    // ワーカーが落ちても、どちらの目印も期限で必ず明ける
    let (pending_ttl, lock_ttl) = queue::remaining_ttl_secs(redis, project_id, REPO_KEY, pr)
        .await
        .expect("ttl");
    assert!(
        pending_ttl > 0 && pending_ttl <= queue::SUMMARY_PENDING_TTL_SECS as i64,
        "印に TTL が付いている: {pending_ttl}"
    );
    assert!(
        lock_ttl > 0 && lock_ttl <= queue::SUMMARY_LOCK_TTL_SECS as i64,
        "ロックに TTL が付いている: {lock_ttl}"
    );

    // 保持中は取れない
    assert!(
        queue::try_acquire_update_lock(redis, project_id, REPO_KEY, pr)
            .await
            .expect("acquire lock")
            .is_none(),
        "同じ PR の更新は同時に走らない"
    );

    // 期限切れで別のジョブが取り直した状況を作る
    assert!(
        queue::release_update_lock(redis, project_id, REPO_KEY, pr, &token)
            .await
            .expect("release lock")
    );
    let newer = queue::try_acquire_update_lock(redis, project_id, REPO_KEY, pr)
        .await
        .expect("acquire lock")
        .expect("解放後は取り直せる");

    // 遅れて完走した古いジョブは、取り直されたロックを解放しない
    assert!(
        !queue::release_update_lock(redis, project_id, REPO_KEY, pr, &token)
            .await
            .expect("release lock"),
        "古いトークンでは解放できない"
    );
    assert!(
        queue::try_acquire_update_lock(redis, project_id, REPO_KEY, pr)
            .await
            .expect("acquire lock")
            .is_none(),
        "取り直したロックはそのまま残る"
    );

    queue::release_update_lock(redis, project_id, REPO_KEY, pr, &newer)
        .await
        .expect("release lock");
    queue::clear_pending(redis, project_id, REPO_KEY, pr)
        .await
        .expect("clear pending");
    assert!(
        queue::try_mark_pending(redis, project_id, REPO_KEY, pr)
            .await
            .expect("mark pending"),
        "印を落とせば次の更新をまた積める"
    );
    queue::clear_pending(redis, project_id, REPO_KEY, pr)
        .await
        .expect("clear pending");
}

/// 同じ PR を更新中のジョブがいる間は投稿せず、少し後ろへ積み直す。
///
/// 合流はジョブの本数を減らすだけで同時実行は止まらない。並行して走ると、古い状態を
/// 読んだ側の書き込みが後から着いてコメントが巻き戻る（仕様 §7）。
///
/// 「自分の番ではない」をジョブの失敗として返すと、`RetryPolicy` にバックオフが無い
/// ぶん数ミリ秒で試行回数を使い切って終端する。そのとき「更新待ち」の印だけが残り、
/// 生きたジョブが 1 本も無いまま、印の TTL のあいだ以降の遷移が合流で捨てられる。
#[serial_test::serial]
#[tokio::test]
async fn a_concurrent_summary_update_is_retried_instead_of_overwriting() {
    let mock_server = MockServer::start().await;
    // SAFETY: serial アトリビュートにより他テストとの並列実行を防いでいる。
    unsafe {
        std::env::set_var("GITHUB_API_BASE_URL", mock_server.uri());
    }
    let mut app = TestApp::new_with_github().await;
    let reviewer = app.insert_user_default().await;
    let tp = app.insert_tenant_project(reviewer.id).await;
    seed_statuses(&app, tp.project_id).await;
    link_integration(&app, tp.project_id, reviewer.id).await;
    let marker = service::github::pr_comments::summary_marker(tp.project_id);
    mount_mocks(&mock_server, true, &marker).await;

    app.reset_session_client();
    app.login_session_no_content(&reviewer.email, &reviewer.password)
        .await;

    let res = app
        .post_json_with_session(
            &format!(
                "/v1/tenants/{}/projects/{}/reviews",
                tp.tenant_id, tp.project_id
            ),
            serde_json::json!({
                "pr_number": PR_NUMBER,
                "head_sha": "60cdd7795f94fa4e4148ce996c2efb4c363e3f5e",
                "summary": "総評",
                "findings": [{ "severity": "high", "title": "認可漏れ", "body": "本文" }],
            }),
        )
        .await;
    assert_eq!(res.status(), StatusCode::CREATED);

    // 先行するジョブがロックを握っている状態を作る
    let held = service::github::review_summary_queue::try_acquire_update_lock(
        &app.state.redis_client,
        tp.project_id,
        REPO_KEY,
        PR_NUMBER,
    )
    .await
    .expect("acquire lock")
    .expect("先行ジョブがロックを取れる");

    let state = job_state(&app);
    let job = job::ReviewSummaryJob {
        project_id: tp.project_id,
        pr_number: PR_NUMBER,
    };
    let queued_before = queued_summary_jobs(&app, tp.project_id).await;
    job::review_summary::process(job.clone(), apalis::prelude::Data::new(state.clone()))
        .await
        .expect("自分の番でないだけなので失敗にしない（失敗にすると即時再試行を使い切る）");
    assert!(
        bodies_of(&mock_server, wiremock::http::Method::PATCH)
            .await
            .is_empty(),
        "ロックを取れなかったジョブは投稿しない"
    );
    assert_eq!(
        queued_summary_jobs(&app, tp.project_id).await,
        queued_before + 1,
        "拾い直すジョブを積み直す（積まないと、この更新はどこにも残らない）"
    );
    assert!(
        !service::github::review_summary_queue::try_mark_pending(
            &app.state.redis_client,
            tp.project_id,
            REPO_KEY,
            PR_NUMBER
        )
        .await
        .expect("pending flag"),
        "積み直したジョブに合流させるため「更新待ち」の印は立っている"
    );

    // 先行ジョブが終われば、次のジョブが最新状態で更新できる
    service::github::review_summary_queue::release_update_lock(
        &app.state.redis_client,
        tp.project_id,
        REPO_KEY,
        PR_NUMBER,
        &held,
    )
    .await
    .expect("release lock");

    job::review_summary::process(job, apalis::prelude::Data::new(state))
        .await
        .expect("post review summary");
    let patched = bodies_of(&mock_server, wiremock::http::Method::PATCH).await;
    assert_eq!(patched.len(), 1, "解放後は 1 回だけ更新する");
    assert!(
        patched[0]["body"]
            .as_str()
            .expect("comment body")
            .contains("| High | Open | 1 |"),
        "最新の件数が出る: {}",
        patched[0]["body"]
    );

    app.cleanup_user(reviewer.id).await;
}

/// 第三者が同じマーカーのコメントを先に置いていても、それを更新しに行かない。
///
/// マーカーは PR の参加者なら誰でも本文に書ける。App は他人のコメントを編集できないので、
/// 掴んでしまうと更新が失敗し続け、失敗はベストエフォートで握り潰されるため正式な要約が
/// 永久に作られない（仕様 §7）。
#[serial_test::serial]
#[tokio::test]
async fn a_third_party_marker_does_not_hijack_the_summary_comment() {
    let mock_server = MockServer::start().await;
    // SAFETY: serial アトリビュートにより他テストとの並列実行を防いでいる。
    unsafe {
        std::env::set_var("GITHUB_API_BASE_URL", mock_server.uri());
    }

    let mut app = TestApp::new_with_github().await;
    let reviewer = app.insert_user_default().await;
    let tp = app.insert_tenant_project(reviewer.id).await;
    seed_statuses(&app, tp.project_id).await;
    link_integration(&app, tp.project_id, reviewer.id).await;
    let marker = service::github::pr_comments::summary_marker(tp.project_id);

    // 共通のモックを敷いたうえで、一覧だけ「第三者が先取りしたコメント」に差し替える
    mount_mocks(&mock_server, false, &marker).await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/repos/{REPO_OWNER}/{REPO_NAME}/issues/{PR_NUMBER}/comments"
        )))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!([{
                "id": 999,
                "body": format!("{}\nにせの要約", marker),
                "user": { "login": "someone", "type": "User" }
            }])),
        )
        .mount(&mock_server)
        .await;

    app.reset_session_client();
    app.login_session_no_content(&reviewer.email, &reviewer.password)
        .await;
    let res = app
        .post_json_with_session(
            &format!(
                "/v1/tenants/{}/projects/{}/reviews",
                tp.tenant_id, tp.project_id
            ),
            serde_json::json!({
                "pr_number": PR_NUMBER,
                "head_sha": REVIEWED_HEAD,
                "summary": "総評",
                "findings": [],
            }),
        )
        .await;
    assert_eq!(res.status(), StatusCode::CREATED);

    let state = job_state(&app);
    job::review_summary::process(
        job::ReviewSummaryJob {
            project_id: tp.project_id,
            pr_number: PR_NUMBER,
        },
        apalis::prelude::Data::new(state),
    )
    .await
    .expect("post review summary");

    // 第三者のコメント（999）を編集しに行かず、自分のコメントを新規に作る
    let patched = bodies_of(&mock_server, wiremock::http::Method::PATCH).await;
    assert!(
        patched.is_empty(),
        "他人のコメントを更新しようとしない: {patched:?}"
    );
    let posted = bodies_of(&mock_server, wiremock::http::Method::POST).await;
    let posted: Vec<&serde_json::Value> =
        posted.iter().filter(|b| b.get("body").is_some()).collect();
    assert_eq!(posted.len(), 1, "自分の要約を新規に作る");

    app.cleanup_user(reviewer.id).await;
}

/// GitHub 連携の無いプロジェクトでは投稿しない（起票・管理自体は task 側で完結する）。
#[serial_test::serial]
#[tokio::test]
async fn a_project_without_integration_skips_the_comment() {
    let mock_server = MockServer::start().await;
    // SAFETY: serial アトリビュートにより他テストとの並列実行を防いでいる。
    unsafe {
        std::env::set_var("GITHUB_API_BASE_URL", mock_server.uri());
    }
    let mut app = TestApp::new_with_github().await;
    let reviewer = app.insert_user_default().await;
    let tp = app.insert_tenant_project(reviewer.id).await;
    seed_statuses(&app, tp.project_id).await;
    // 連携は張らない
    let marker = service::github::pr_comments::summary_marker(tp.project_id);
    mount_mocks(&mock_server, false, &marker).await;

    app.reset_session_client();
    app.login_session_no_content(&reviewer.email, &reviewer.password)
        .await;

    let res = app
        .post_json_with_session(
            &format!(
                "/v1/tenants/{}/projects/{}/reviews",
                tp.tenant_id, tp.project_id
            ),
            serde_json::json!({
                "pr_number": PR_NUMBER,
                "head_sha": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "findings": [],
            }),
        )
        .await;
    assert_eq!(res.status(), StatusCode::CREATED, "起票自体は成功する");

    job::review_summary::process(
        job::ReviewSummaryJob {
            project_id: tp.project_id,
            pr_number: PR_NUMBER,
        },
        apalis::prelude::Data::new(job_state(&app)),
    )
    .await
    .expect("job succeeds without an integration");

    let requests = mock_server
        .received_requests()
        .await
        .expect("received requests");
    assert!(
        requests.is_empty(),
        "連携が無ければ GitHub API を叩かない: {} 件",
        requests.len()
    );

    app.cleanup_user(reviewer.id).await;
}
