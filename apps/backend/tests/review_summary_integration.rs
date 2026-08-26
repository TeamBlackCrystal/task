mod common;

use axum::http::StatusCode;
use common::TestApp;
use entity::{github_integrations, project_statuses};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, prelude::Uuid};
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

fn unique_installation_id() -> i64 {
    // 同じ DB を共有する他テストと衝突しない範囲で散らす
    (Uuid::new_v4().as_u128() % 1_000_000) as i64 + 8_000_000
}

async fn mount_mocks(server: &MockServer, existing_comment: bool) {
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
            "user": { "login": "yupix" }
        })))
        .mount(server)
        .await;

    // 既存コメントの有無で「新規投稿」と「編集」を出し分ける
    let comments = if existing_comment {
        serde_json::json!([
            { "id": 1, "body": "無関係なコメント" },
            {
                "id": COMMENT_ID,
                "body": format!("{}\n古い要約", service::github::pr_comments::SUMMARY_MARKER)
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

fn job_state(app: &TestApp) -> job::JobState {
    job::JobState {
        settings: app.state.settings.clone(),
        db: app.state.db.clone(),
        redis_client: app.state.redis_client.clone(),
        smtp_client: app.state.smtp_client.clone(),
        http_client: app.state.http_client.clone(),
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
    mount_mocks(&mock_server, false).await;

    let mut app = TestApp::new_with_github().await;
    let reviewer = app.insert_user_default().await;
    let tp = app.insert_tenant_project(reviewer.id).await;
    seed_statuses(&app, tp.project_id).await;
    link_integration(&app, tp.project_id, reviewer.id).await;

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
    assert!(
        body.starts_with(service::github::pr_comments::SUMMARY_MARKER),
        "マーカーが行頭にある: {body}"
    );
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
    mount_mocks(&mock_server, true).await;

    let mut app = TestApp::new_with_github().await;
    let reviewer = app.insert_user_default().await;
    let tp = app.insert_tenant_project(reviewer.id).await;
    seed_statuses(&app, tp.project_id).await;
    link_integration(&app, tp.project_id, reviewer.id).await;

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
                "head_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
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
    assert!(body.starts_with(service::github::pr_comments::SUMMARY_MARKER));
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
    mount_mocks(&mock_server, false).await;

    let mut app = TestApp::new_with_github().await;
    let reviewer = app.insert_user_default().await;
    let tp = app.insert_tenant_project(reviewer.id).await;
    seed_statuses(&app, tp.project_id).await;
    link_integration(&app, tp.project_id, reviewer.id).await;

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
            PR_NUMBER
        )
        .await
        .expect("pending flag"),
        "ジョブの実行後は次の更新を受け付ける"
    );

    app.cleanup_user(reviewer.id).await;
}

/// GitHub 連携の無いプロジェクトでは投稿しない（起票・管理自体は成功している）。
#[serial_test::serial]
#[tokio::test]
async fn a_project_without_integration_skips_the_comment() {
    let mock_server = MockServer::start().await;
    // SAFETY: serial アトリビュートにより他テストとの並列実行を防いでいる。
    unsafe {
        std::env::set_var("GITHUB_API_BASE_URL", mock_server.uri());
    }
    mount_mocks(&mock_server, false).await;

    let mut app = TestApp::new_with_github().await;
    let reviewer = app.insert_user_default().await;
    let tp = app.insert_tenant_project(reviewer.id).await;
    seed_statuses(&app, tp.project_id).await;
    // 連携は張らない

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
