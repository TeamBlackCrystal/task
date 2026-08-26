mod common;

use axum::http::StatusCode;
use common::TestApp;
use entity::{project_statuses, tasks};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait, prelude::Uuid};

// レビュー指摘管理（#623 の仕様）の統合テスト。
//
// 状態遷移の表そのものは service の単体テストが固定するので、ここでは
// 「API を通したときに規則どおり通る／弾かれる」ことと、繰り延べ先タスクの
// 作成・クローズという DB をまたぐ副作用を確かめる。

struct Fixture {
    app: TestApp,
    tenant_id: Uuid,
    project_id: Uuid,
    reviewer: common::TestUser,
    developer: common::TestUser,
}

impl Fixture {
    fn reviews_path(&self) -> String {
        format!(
            "/v1/tenants/{}/projects/{}/reviews",
            self.tenant_id, self.project_id
        )
    }

    fn findings_path(&self) -> String {
        format!(
            "/v1/tenants/{}/projects/{}/review-findings",
            self.tenant_id, self.project_id
        )
    }

    async fn login(&mut self, user: &common::TestUser) {
        self.app.reset_session_client();
        self.app
            .login_session_no_content(&user.email, &user.password)
            .await;
    }
}

/// レビュワー（テナントオーナー）と修正者（テナントメンバー）がいるプロジェクト。
/// 繰り延べ先タスクの作成に必要な既定ステータスと完了ステータスも用意する。
async fn setup() -> Fixture {
    let mut app = TestApp::new().await;
    let reviewer = app.insert_user_default().await;
    let developer = app.insert_user_default().await;
    let tp = app.insert_tenant_project(reviewer.id).await;

    for (name, position, is_default, is_done) in
        [("Todo", 0, true, false), ("Done", 1, false, true)]
    {
        project_statuses::ActiveModel {
            id: Set(Uuid::new_v4()),
            project_id: Set(tp.project_id),
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

    // 修正者をテナントメンバーにして、プロジェクトへ入れるようにする
    app.reset_session_client();
    app.login_session_no_content(&reviewer.email, &reviewer.password)
        .await;
    let added = app
        .post_json_with_session(
            &format!("/v1/tenants/{}/members", tp.tenant_id),
            serde_json::json!({ "user_id": developer.id, "role": "Member" }),
        )
        .await;
    assert_eq!(added.status(), StatusCode::CREATED);

    Fixture {
        app,
        tenant_id: tp.tenant_id,
        project_id: tp.project_id,
        reviewer,
        developer,
    }
}

async fn json(res: reqwest::Response) -> serde_json::Value {
    res.json::<serde_json::Value>().await.expect("json body")
}

/// 指摘 1 件のラウンドを起票し、(review_id, finding_id) を返す。
async fn submit_round(fx: &Fixture, pr: i32, severity: &str, title: &str) -> (String, String) {
    let res = fx
        .app
        .post_json_with_session(
            &fx.reviews_path(),
            serde_json::json!({
                "pr_number": pr,
                "head_sha": "60cdd7795f94fa4e4148ce996c2efb4c363e3f5e",
                "summary": "総評",
                "findings": [{
                    "severity": severity,
                    "title": title,
                    "body": "再現条件と根拠",
                    "file": "apps/frontend/src/App.vue",
                    "line": 42,
                }],
            }),
        )
        .await;
    assert_eq!(
        res.status(),
        StatusCode::CREATED,
        "ラウンドの起票は成功する"
    );
    let body = json(res).await;
    (
        body["id"].as_str().expect("review id").to_string(),
        body["findings"][0]["id"]
            .as_str()
            .expect("finding id")
            .to_string(),
    )
}

async fn transition(fx: &Fixture, finding_id: &str, state: &str) -> reqwest::Response {
    fx.app
        .patch_json_with_session(
            &format!("{}/{finding_id}", fx.findings_path()),
            serde_json::json!({ "state": state }),
        )
        .await
}

/// 一括起票 → 一覧 → fixed → verified の正常系と、集計のマージ可否。
#[tokio::test]
async fn round_is_created_and_findings_run_through_the_happy_path() {
    let mut fx = setup().await;
    fx.login(&fx.reviewer.clone()).await;

    let (review_id, finding_id) = submit_round(&fx, 618, "high", "認可が抜けている").await;

    // 起票直後は High が 1 件未解決なのでマージ不可
    let summary = json(
        fx.app
            .get_with_session(&format!("{}/summary?pr=618", fx.reviews_path()))
            .await,
    )
    .await;
    assert_eq!(summary["rounds"], 1);
    assert_eq!(summary["blocking"], 1);
    assert_eq!(summary["mergeable"], false);

    // ラウンド一覧に R1 が出る
    let rounds = json(
        fx.app
            .get_with_session(&format!("{}?pr=618", fx.reviews_path()))
            .await,
    )
    .await;
    let rounds = rounds.as_array().expect("rounds");
    assert_eq!(rounds.len(), 1);
    assert_eq!(rounds[0]["round"], 1);
    assert_eq!(rounds[0]["finding_count"], 1);
    assert_eq!(rounds[0]["reviewer"]["id"], fx.reviewer.id.to_string());

    // 修正者が fixed を宣言する
    fx.login(&fx.developer.clone()).await;
    let res = transition(&fx, &finding_id, "fixed").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = json(res).await;
    assert_eq!(body["state"], "fixed");
    assert_eq!(body["fixed_by"], fx.developer.id.to_string());
    // 起票と fixed の 2 行が履歴に残る
    let history = body["transitions"].as_array().expect("transitions");
    assert_eq!(history.len(), 2);
    assert!(history[0]["from_state"].is_null(), "起票は from が null");
    assert_eq!(history[1]["to_state"], "fixed");

    // fixed でもマージ判定は塞がったまま（確認が済んでいないため）
    let summary = json(
        fx.app
            .get_with_session(&format!("{}/summary?pr=618", fx.reviews_path()))
            .await,
    )
    .await;
    assert_eq!(summary["blocking"], 1, "fixed は未解決として数える");

    // レビュワーが確認する
    fx.login(&fx.reviewer.clone()).await;
    let res = transition(&fx, &finding_id, "verified").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json(res).await["state"], "verified");

    let summary = json(
        fx.app
            .get_with_session(&format!("{}/summary?pr=618", fx.reviews_path()))
            .await,
    )
    .await;
    assert_eq!(summary["blocking"], 0);
    assert_eq!(summary["mergeable"], true, "確認済みならマージできる");

    // ラウンド詳細からも読める
    let detail = json(
        fx.app
            .get_with_session(&format!("{}/{review_id}", fx.reviews_path()))
            .await,
    )
    .await;
    assert_eq!(detail["findings"][0]["state"], "verified");

    fx.app.cleanup_user(fx.reviewer.id).await;
    fx.app.cleanup_user(fx.developer.id).await;
}

/// 自分で fixed を宣言した人は、自分でその指摘を verified にできない。
/// 別のレビュワーなら通る（過剰拒否でないことの対照）。
#[tokio::test]
async fn the_fixer_cannot_verify_their_own_fix() {
    let mut fx = setup().await;
    fx.login(&fx.reviewer.clone()).await;
    let (_, finding_id) = submit_round(&fx, 700, "medium", "境界値が 1 つずれている").await;

    // レビュワー自身が fixed を宣言してしまうと…
    let res = transition(&fx, &finding_id, "fixed").await;
    assert_eq!(res.status(), StatusCode::OK);

    // …同じ人は verified に進められない
    let res = transition(&fx, &finding_id, "verified").await;
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "自分の修正を自分で確認済みにはできない"
    );

    // 別のレビュワー（R2 を出した人）なら確認できる
    fx.login(&fx.developer.clone()).await;
    let res = fx
        .app
        .post_json_with_session(
            &fx.reviews_path(),
            serde_json::json!({
                "pr_number": 700,
                "head_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "summary": "2 巡目",
                "findings": [],
            }),
        )
        .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    assert_eq!(
        json(res).await["round"],
        2,
        "同一 PR の再レビューは R2 になる"
    );

    let res = transition(&fx, &finding_id, "verified").await;
    assert_eq!(res.status(), StatusCode::OK, "別のレビュワーなら確認できる");

    fx.app.cleanup_user(fx.reviewer.id).await;
    fx.app.cleanup_user(fx.developer.id).await;
}

/// レビューを一度も出していない利用者は、差し戻し・棄却を行えない。
#[tokio::test]
async fn reviewer_only_transitions_reject_the_developer() {
    let mut fx = setup().await;
    fx.login(&fx.reviewer.clone()).await;
    let (_, finding_id) = submit_round(&fx, 701, "high", "認可が抜けている").await;

    fx.login(&fx.developer.clone()).await;
    // 棄却はレビュー側の判断
    let res = transition(&fx, &finding_id, "rejected").await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // fixed を宣言するのは修正側でよい
    assert_eq!(
        transition(&fx, &finding_id, "fixed").await.status(),
        StatusCode::OK
    );
    // 差し戻し（未対応判定）はレビュー側だけ
    let res = transition(&fx, &finding_id, "open").await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // レビュワーなら差し戻せる
    fx.login(&fx.reviewer.clone()).await;
    let res = transition(&fx, &finding_id, "open").await;
    assert_eq!(res.status(), StatusCode::OK, "レビュワーは差し戻せる");
    assert_eq!(json(res).await["state"], "open");

    fx.app.cleanup_user(fx.reviewer.id).await;
    fx.app.cleanup_user(fx.developer.id).await;
}

/// 規則にない遷移は 409。verified は終端。
#[tokio::test]
async fn invalid_transitions_are_rejected() {
    let mut fx = setup().await;
    fx.login(&fx.reviewer.clone()).await;
    let (_, finding_id) = submit_round(&fx, 702, "low", "命名が揺れている").await;

    // 確認を飛ばして verified にはできない
    assert_eq!(
        transition(&fx, &finding_id, "verified").await.status(),
        StatusCode::CONFLICT
    );

    // open → fixed → verified まで進めてから
    assert_eq!(
        transition(&fx, &finding_id, "fixed").await.status(),
        StatusCode::OK
    );
    fx.login(&fx.developer.clone()).await;
    // developer は fixed を宣言していないので確認できる
    assert_eq!(
        transition(&fx, &finding_id, "verified").await.status(),
        StatusCode::FORBIDDEN,
        "レビューを出していない利用者は確認もできない"
    );

    fx.login(&fx.reviewer.clone()).await;
    // reviewer が fixed を宣言したので、この指摘は本人以外が確認するしかない。
    // 2 巡目を developer が出して確認する
    fx.login(&fx.developer.clone()).await;
    assert_eq!(
        fx.app
            .post_json_with_session(
                &fx.reviews_path(),
                serde_json::json!({
                    "pr_number": 702,
                    "head_sha": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                    "findings": [],
                }),
            )
            .await
            .status(),
        StatusCode::CREATED
    );
    assert_eq!(
        transition(&fx, &finding_id, "verified").await.status(),
        StatusCode::OK
    );

    // verified は終端。どこへも戻せない
    for state in ["open", "fixed", "deferred", "rejected"] {
        assert_eq!(
            transition(&fx, &finding_id, state).await.status(),
            StatusCode::CONFLICT,
            "verified からは {state} へ遷移できない"
        );
    }

    fx.app.cleanup_user(fx.reviewer.id).await;
    fx.app.cleanup_user(fx.developer.id).await;
}

/// 繰り延べで通常タスクが起票されリンクされ、取り消すとそのタスクが完了する。
#[tokio::test]
async fn deferring_creates_a_task_and_reverting_closes_it() {
    let mut fx = setup().await;
    fx.login(&fx.reviewer.clone()).await;
    let (_, finding_id) = submit_round(&fx, 703, "nit", "コメントの表記ゆれ").await;

    let res = transition(&fx, &finding_id, "deferred").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = json(res).await;
    assert_eq!(body["state"], "deferred");
    let task_id: Uuid = body["deferred_task_id"]
        .as_str()
        .expect("繰り延べ先タスクがリンクされる")
        .parse()
        .expect("uuid");

    let task = tasks::Entity::find_by_id(task_id)
        .one(&fx.app.state.db)
        .await
        .expect("query task")
        .expect("task row");
    assert_eq!(task.project_id, fx.project_id, "同じプロジェクトに起票する");
    assert!(
        task.title.contains("コメントの表記ゆれ"),
        "指摘のタイトルを引き継ぐ: {}",
        task.title
    );
    assert_eq!(task.priority, tasks::TaskPriority::Low);
    assert!(task.completed_at.is_none(), "起票直後は未完了");

    // 繰り延べた Low/Nit はマージを塞がない
    let summary = json(
        fx.app
            .get_with_session(&format!("{}/summary?pr=703", fx.reviews_path()))
            .await,
    )
    .await;
    assert_eq!(summary["mergeable"], true);

    // 「やはり今直す」で open へ戻すと、自動起票したタスクは畳まれる
    let res = transition(&fx, &finding_id, "open").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = json(res).await;
    assert_eq!(body["state"], "open");
    assert!(body["deferred_task_id"].is_null(), "リンクは外れる: {body}");

    let task = tasks::Entity::find_by_id(task_id)
        .one(&fx.app.state.db)
        .await
        .expect("query task")
        .expect("task row");
    assert!(
        task.completed_at.is_some(),
        "二重管理を作らないよう自動起票タスクは完了する"
    );

    fx.app.cleanup_user(fx.reviewer.id).await;
    fx.app.cleanup_user(fx.developer.id).await;
}

/// マージ前必須の重大度は繰り延べられない。
///
/// 繰り延べはマージ可否の集計から外れるので、High / Medium に許すと
/// 「1 回 deferred にすればマージ可」という迂回路ができる（仕様 §3）。
#[tokio::test]
async fn high_and_medium_findings_cannot_be_deferred() {
    let mut fx = setup().await;
    fx.login(&fx.reviewer.clone()).await;

    let res = fx
        .app
        .post_json_with_session(
            &fx.reviews_path(),
            serde_json::json!({
                "pr_number": 708,
                "head_sha": "60cdd7795f94fa4e4148ce996c2efb4c363e3f5e",
                "summary": "総評",
                "findings": [
                    { "severity": "high", "title": "認可が抜けている", "body": "根拠" },
                    { "severity": "medium", "title": "境界値が 1 つずれている", "body": "根拠" },
                    { "severity": "low", "title": "命名が揺れている", "body": "根拠" },
                ],
            }),
        )
        .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let body = json(res).await;
    let id_of = |severity: &str| -> String {
        body["findings"]
            .as_array()
            .expect("findings")
            .iter()
            .find(|f| f["severity"] == severity)
            .unwrap_or_else(|| panic!("{severity} の指摘"))["id"]
            .as_str()
            .expect("finding id")
            .to_string()
    };

    for severity in ["high", "medium"] {
        let finding_id = id_of(severity);
        let res = transition(&fx, &finding_id, "deferred").await;
        assert_eq!(
            res.status(),
            StatusCode::CONFLICT,
            "{severity} は繰り延べられない"
        );

        // 状態も変わっていない（拒否したのにタスクだけ起票される、を防ぐ）
        let listed = json(
            fx.app
                .get_with_session(&format!("{}?pr=708", fx.findings_path()))
                .await,
        )
        .await;
        let finding = listed
            .as_array()
            .expect("findings")
            .iter()
            .find(|f| f["id"] == finding_id.as_str())
            .expect("対象の指摘")
            .clone();
        assert_eq!(finding["state"], "open");
        assert!(
            finding["deferred_task_id"].is_null(),
            "拒否した繰り延べでタスクを起票しない: {finding}"
        );
    }

    // マージ可否は「可」に変わらない
    let summary = json(
        fx.app
            .get_with_session(&format!("{}/summary?pr=708", fx.reviews_path()))
            .await,
    )
    .await;
    assert_eq!(summary["mergeable"], false);
    assert_eq!(summary["blocking"], 2, "High / Medium は未解決のまま");

    // 対照: Low は繰り延べられ、通常タスクが起票される
    let res = transition(&fx, &id_of("low"), "deferred").await;
    assert_eq!(res.status(), StatusCode::OK, "Low は繰り延べられる");
    let body = json(res).await;
    assert_eq!(body["state"], "deferred");
    assert!(
        body["deferred_task_id"].is_string(),
        "繰り延べ先タスクがリンクされる: {body}"
    );

    fx.app.cleanup_user(fx.reviewer.id).await;
    fx.app.cleanup_user(fx.developer.id).await;
}

/// 指摘ゼロのラウンドも正当（「指摘なし」の記録）。
#[tokio::test]
async fn a_round_without_findings_is_valid() {
    let mut fx = setup().await;
    fx.login(&fx.reviewer.clone()).await;

    let res = fx
        .app
        .post_json_with_session(
            &fx.reviews_path(),
            serde_json::json!({
                "pr_number": 704,
                "head_sha": "cccccccccccccccccccccccccccccccccccccccc",
                "summary": "具体的な不具合は見つからなかった",
                "findings": [],
            }),
        )
        .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let body = json(res).await;
    assert_eq!(body["round"], 1);
    assert_eq!(body["finding_count"], 0);
    assert_eq!(body["summary"], "具体的な不具合は見つからなかった");

    let summary = json(
        fx.app
            .get_with_session(&format!("{}/summary?pr=704", fx.reviews_path()))
            .await,
    )
    .await;
    assert_eq!(summary["rounds"], 1);
    assert_eq!(summary["mergeable"], true);

    fx.app.cleanup_user(fx.reviewer.id).await;
    fx.app.cleanup_user(fx.developer.id).await;
}

/// 指摘一覧は状態・重大度で絞り込め、綴り違いは黙って無視せず 400 にする。
#[tokio::test]
async fn findings_can_be_filtered_and_unknown_filters_are_rejected() {
    let mut fx = setup().await;
    fx.login(&fx.reviewer.clone()).await;

    let res = fx
        .app
        .post_json_with_session(
            &fx.reviews_path(),
            serde_json::json!({
                "pr_number": 705,
                "head_sha": "dddddddddddddddddddddddddddddddddddddddd",
                "findings": [
                    { "severity": "high", "title": "認可漏れ", "body": "本文" },
                    { "severity": "low", "title": "命名", "body": "本文" },
                    { "severity": "nit", "title": "表記", "body": "本文" },
                ],
            }),
        )
        .await;
    assert_eq!(res.status(), StatusCode::CREATED);

    let all = json(
        fx.app
            .get_with_session(&format!("{}?pr=705", fx.findings_path()))
            .await,
    )
    .await;
    assert_eq!(all.as_array().expect("findings").len(), 3);

    let filtered = json(
        fx.app
            .get_with_session(&format!("{}?pr=705&severity=low,nit", fx.findings_path()))
            .await,
    )
    .await;
    let filtered = filtered.as_array().expect("findings");
    assert_eq!(filtered.len(), 2, "重大度で絞り込める");
    assert!(
        filtered
            .iter()
            .all(|f| f["severity"] == "low" || f["severity"] == "nit")
    );

    let open_only = json(
        fx.app
            .get_with_session(&format!("{}?pr=705&state=open", fx.findings_path()))
            .await,
    )
    .await;
    assert_eq!(open_only.as_array().expect("findings").len(), 3);
    let verified_only = json(
        fx.app
            .get_with_session(&format!("{}?pr=705&state=verified", fx.findings_path()))
            .await,
    )
    .await;
    assert!(verified_only.as_array().expect("findings").is_empty());

    // 綴り違いを黙って無視すると絞り込みが効いていないことに気づけない
    assert_eq!(
        fx.app
            .get_with_session(&format!("{}?pr=705&severity=critical", fx.findings_path()))
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );

    fx.app.cleanup_user(fx.reviewer.id).await;
    fx.app.cleanup_user(fx.developer.id).await;
}

/// 他プロジェクト・他テナントの指摘は触れず、存在も漏らさない。
#[tokio::test]
async fn findings_are_scoped_to_their_project() {
    let mut fx = setup().await;
    fx.login(&fx.reviewer.clone()).await;
    let (review_id, finding_id) = submit_round(&fx, 706, "high", "認可漏れ").await;

    // 同じテナントの別プロジェクト経由では見えない
    let other = fx.app.insert_tenant_project(fx.reviewer.id).await;
    let other_reviews = format!(
        "/v1/tenants/{}/projects/{}/reviews",
        other.tenant_id, other.project_id
    );
    assert_eq!(
        fx.app
            .get_with_session(&format!("{other_reviews}/{review_id}"))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        fx.app
            .patch_json_with_session(
                &format!(
                    "/v1/tenants/{}/projects/{}/review-findings/{finding_id}",
                    other.tenant_id, other.project_id
                ),
                serde_json::json!({ "state": "fixed" }),
            )
            .await
            .status(),
        StatusCode::NOT_FOUND
    );

    // 対照: 本来のプロジェクト経由なら通る
    assert_eq!(
        fx.app
            .get_with_session(&format!("{}/{review_id}", fx.reviews_path()))
            .await
            .status(),
        StatusCode::OK
    );

    fx.app.cleanup_user(fx.reviewer.id).await;
    fx.app.cleanup_user(fx.developer.id).await;
}

/// 未ログインは 401。テナントに入れない利用者は 403。
#[tokio::test]
async fn access_requires_a_session_and_tenant_membership() {
    let mut fx = setup().await;
    fx.login(&fx.reviewer.clone()).await;
    submit_round(&fx, 707, "high", "認可漏れ").await;

    fx.app.reset_session_client();
    assert_eq!(
        fx.app
            .get_with_session(&format!("{}?pr=707", fx.reviews_path()))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );

    let outsider = fx.app.insert_user_default().await;
    fx.login(&outsider.clone()).await;
    assert_eq!(
        fx.app
            .get_with_session(&format!("{}?pr=707", fx.reviews_path()))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );

    fx.app.cleanup_user(fx.reviewer.id).await;
    fx.app.cleanup_user(fx.developer.id).await;
    fx.app.cleanup_user(outsider.id).await;
}
