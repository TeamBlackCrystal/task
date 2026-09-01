//! レビュー指摘のコマンド。仕様は `docs/features/review-findings.md` §6。
//!
//! ゲートとして使われるので、終了コードと「送信前に弾いたか」を軸に確かめる。

mod common;

use common::*;
use serde_json::json;
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

async fn mount_project_lookup(harness: &Harness) {
    Mock::given(method("GET"))
        .and(path(format!("/v1/tenants/{TENANT}/projects")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([project_json()])))
        .mount(&harness.server)
        .await;
}

fn write_round(dir: &std::path::Path, body: serde_json::Value) -> String {
    let file = dir.join("findings.json");
    std::fs::write(&file, body.to_string()).unwrap();
    file.to_string_lossy().into_owned()
}

#[tokio::test]
async fn submit_reads_the_file_and_creates_the_round_in_one_call() {
    let harness = harness().await;
    mount_project_lookup(&harness).await;
    let dir = tempfile::tempdir().unwrap();
    let file = write_round(
        dir.path(),
        json!({
            "pr": 618,
            "head_sha": HEAD_SHA,
            "findings": [{ "severity": "high", "title": "t", "body": "b" }],
        }),
    );

    Mock::given(method("POST"))
        .and(path(project_path("reviews")))
        .and(body_json(json!({
            "pr_number": 618,
            "head_sha": HEAD_SHA,
            "summary": "",
            "findings": [{
                "severity": "high",
                "title": "t",
                "body": "b",
                "file": null,
                "line": null,
            }],
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": "55555555-5555-4555-8555-555555555555",
            "project_id": PROJECT_ID,
            "pr_number": 618,
            "round": 1,
            "head_sha": HEAD_SHA,
            "reviewer": { "id": "77777777-7777-4777-8777-777777777777", "username": "yupix", "avatar_url": null },
            "reviewer_left_tenant": false,
            "summary": "",
            "pr_title": null,
            "pr_author": null,
            "created_at": "2026-01-01T00:00:00Z",
            "finding_count": 1,
            "findings": [finding_json()],
        })))
        .expect(1)
        .mount(&harness.server)
        .await;

    let code = harness
        .run(&["task", "review", "submit", &file, "--project", "APP"])
        .await
        .unwrap();
    assert_eq!(code, 0);
}

#[tokio::test]
async fn submit_lets_the_pr_option_override_the_json() {
    let harness = harness().await;
    mount_project_lookup(&harness).await;
    let dir = tempfile::tempdir().unwrap();
    let file = write_round(dir.path(), json!({ "pr": 618, "head_sha": HEAD_SHA }));

    Mock::given(method("POST"))
        .and(path(project_path("reviews")))
        .and(body_json(json!({
            "pr_number": 999,
            "head_sha": HEAD_SHA,
            "summary": "",
            "findings": [],
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": "55555555-5555-4555-8555-555555555555",
            "project_id": PROJECT_ID,
            "pr_number": 999,
            "round": 1,
            "head_sha": HEAD_SHA,
            "reviewer": { "id": "77777777-7777-4777-8777-777777777777", "username": "yupix", "avatar_url": null },
            "reviewer_left_tenant": false,
            "summary": "",
            "pr_title": null,
            "pr_author": null,
            "created_at": "2026-01-01T00:00:00Z",
            "finding_count": 0,
            "findings": [],
        })))
        .expect(1)
        .mount(&harness.server)
        .await;

    let code = harness
        .run(&[
            "task",
            "review",
            "submit",
            &file,
            "--project",
            "APP",
            "--pr",
            "999",
        ])
        .await
        .unwrap();
    assert_eq!(code, 0);
}

#[tokio::test]
async fn submit_rejects_a_shortened_head_sha_before_sending() {
    let harness = harness().await;
    let dir = tempfile::tempdir().unwrap();
    let file = write_round(dir.path(), json!({ "pr": 618, "head_sha": "60cdd77" }));

    let err = harness
        .run(&["task", "review", "submit", &file, "--project", "APP"])
        .await
        .unwrap_err();

    assert_eq!(err.exit_code, 2);
    assert!(
        err.message.contains("40-character commit SHA"),
        "{}",
        err.message
    );
    assert!(harness.sent_nothing().await);
}

#[tokio::test]
async fn submit_reports_invalid_json_with_the_source_it_came_from() {
    let harness = harness().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("findings.json");
    std::fs::write(&file, "{ not json").unwrap();

    let err = harness
        .run(&[
            "task",
            "review",
            "submit",
            file.to_str().unwrap(),
            "--project",
            "APP",
        ])
        .await
        .unwrap_err();

    assert_eq!(err.exit_code, 2);
    assert!(err.message.contains("invalid JSON in"), "{}", err.message);
}

#[tokio::test]
async fn list_sends_the_state_and_severity_filters() {
    let harness = harness().await;
    mount_project_lookup(&harness).await;
    Mock::given(method("GET"))
        .and(path(project_path("review-findings")))
        .and(query_param("pr", "618"))
        .and(query_param("state", "open,fixed"))
        .and(query_param("severity", "high"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([finding_json()])))
        .expect(1)
        .mount(&harness.server)
        .await;

    let code = harness
        .run(&[
            "task",
            "review",
            "list",
            "--project",
            "APP",
            "--pr",
            "618",
            "--state",
            "open,fixed",
            "--severity",
            "high",
        ])
        .await
        .unwrap();
    assert_eq!(code, 0);
}

/// 連携を差し替えると旧リポジトリのラウンドが既定の視界から外れる。読み取りの
/// 3 コマンドすべてから過去の連携先を指せること（仕様 §5）。
#[tokio::test]
async fn every_read_command_can_point_at_a_previous_repository() {
    let cases: [(&str, &str, Vec<&str>, serde_json::Value); 3] = [
        ("list", "review-findings", vec![], json!([finding_json()])),
        ("rounds", "reviews", vec![], json!([])),
        (
            "summary",
            "reviews/summary",
            vec!["--no-head-check"],
            summary_json(1, 0, Some("acme/old"), true),
        ),
    ];

    for (command, suffix, extra, body) in cases {
        let harness = harness().await;
        mount_project_lookup(&harness).await;
        Mock::given(method("GET"))
            .and(path(project_path(suffix)))
            .and(query_param("pr", "618"))
            .and(query_param("repo", "acme/old"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .expect(1)
            .mount(&harness.server)
            .await;

        let mut args = vec![
            "task",
            "review",
            command,
            "--project",
            "APP",
            "--pr",
            "618",
            "--repo",
            "acme/old",
        ];
        args.extend(extra);
        assert_eq!(harness.run(&args).await.unwrap(), 0, "{command}");
    }
}

/// 連携を張る前に溜めたラウンドはサーバー側で空文字列として残る。空文字を
/// 「未指定」に丸めると、そこへ到達する手段が無くなる。
#[tokio::test]
async fn an_empty_repository_selects_the_rounds_recorded_before_the_integration() {
    let harness = harness().await;
    mount_project_lookup(&harness).await;
    Mock::given(method("GET"))
        .and(path(project_path("review-findings")))
        .and(query_param("pr", "618"))
        .and(query_param("repo", ""))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&harness.server)
        .await;

    let code = harness
        .run(&[
            "task",
            "review",
            "list",
            "--project",
            "APP",
            "--pr",
            "618",
            "--repo",
            "",
        ])
        .await
        .unwrap();
    assert_eq!(code, 0);
}

#[tokio::test]
async fn a_repository_that_is_not_owner_name_is_rejected_before_sending() {
    for value in ["acme", "acme/", "/old", "acme/old/extra"] {
        let harness = harness().await;
        let err = harness
            .run(&[
                "task",
                "review",
                "list",
                "--project",
                "APP",
                "--pr",
                "618",
                "--repo",
                value,
            ])
            .await
            .unwrap_err();

        assert_eq!(err.exit_code, 2, "{value}");
        assert!(
            err.message.starts_with("--repo must be owner/name"),
            "{value}: {}",
            err.message
        );
        assert!(harness.sent_nothing().await, "{value}");
    }
}

#[tokio::test]
async fn a_misspelled_filter_is_rejected_before_sending() {
    for (flag, value, expected) in [
        ("--state", "closed", "unknown state"),
        ("--severity", "critical", "unknown severity"),
    ] {
        let harness = harness().await;
        let err = harness
            .run(&[
                "task",
                "review",
                "list",
                "--project",
                "APP",
                "--pr",
                "618",
                flag,
                value,
            ])
            .await
            .unwrap_err();

        assert_eq!(err.exit_code, 2, "{flag}");
        assert!(err.message.starts_with(expected), "{flag}: {}", err.message);
        assert!(harness.sent_nothing().await, "{flag}");
    }
}

#[tokio::test]
async fn a_pr_that_is_not_a_positive_integer_is_rejected_before_sending() {
    let harness = harness().await;
    let err = harness
        .run(&["task", "review", "list", "--project", "APP", "--pr", "abc"])
        .await
        .unwrap_err();

    assert_eq!(err.exit_code, 2);
    assert!(
        err.message.contains("--pr must be a positive integer"),
        "{}",
        err.message
    );
    assert!(harness.sent_nothing().await);
}

#[tokio::test]
async fn resolve_sends_the_new_state_and_the_reason() {
    let harness = harness().await;
    mount_project_lookup(&harness).await;
    Mock::given(method("PATCH"))
        .and(path(project_path("review-findings/finding-1")))
        .and(body_json(
            json!({ "state": "deferred", "note": "後で直す" }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(finding_json()))
        .expect(1)
        .mount(&harness.server)
        .await;

    let code = harness
        .run(&[
            "task",
            "review",
            "resolve",
            "finding-1",
            "--project",
            "APP",
            "--state",
            "deferred",
            "--note",
            "後で直す",
        ])
        .await
        .unwrap();
    assert_eq!(code, 0);
}

#[tokio::test]
async fn resolve_rejects_an_unknown_state_before_sending() {
    let harness = harness().await;
    let err = harness
        .run(&[
            "task",
            "review",
            "resolve",
            "finding-1",
            "--project",
            "APP",
            "--state",
            "done",
        ])
        .await
        .unwrap_err();

    assert_eq!(err.exit_code, 2);
    assert!(
        err.message.starts_with("unknown state: done"),
        "{}",
        err.message
    );
    assert!(harness.sent_nothing().await);
}

/// ゲートの本体。通してよい理由が揃ったときだけ 0 で終わる。
async fn summary_exit_code(body: serde_json::Value, extra: &[&str]) -> i32 {
    let harness = harness().await;
    mount_project_lookup(&harness).await;
    Mock::given(method("GET"))
        .and(path(project_path("reviews/summary")))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&harness.server)
        .await;

    let mut args = vec![
        "task",
        "review",
        "summary",
        "--project",
        "APP",
        "--pr",
        "618",
    ];
    args.extend(extra);
    harness.run(&args).await.unwrap()
}

#[tokio::test]
async fn summary_passes_only_a_reviewed_clean_and_current_pull_request() {
    let clean = summary_json(1, 0, Some("acme/app"), true);
    assert_eq!(summary_exit_code(clean, &["--head", HEAD_SHA]).await, 0);
}

#[tokio::test]
async fn summary_blocks_a_pull_request_with_unresolved_findings() {
    let blocked = summary_json(2, 1, Some("acme/app"), false);
    assert_eq!(summary_exit_code(blocked, &["--head", HEAD_SHA]).await, 1);
}

/// 未レビューと「指摘なし」は違う。件数だけで判定すると、一度も見ていない PR が通る。
#[tokio::test]
async fn summary_blocks_a_pull_request_that_has_never_been_reviewed() {
    let unreviewed = summary_json(0, 0, Some("acme/app"), false);
    assert_eq!(
        summary_exit_code(unreviewed, &["--head", HEAD_SHA]).await,
        1
    );
}

/// 連携を外すと集計の視界が空になり、空のラウンド 1 本で「可」を作れてしまう。
#[tokio::test]
async fn summary_blocks_a_project_without_an_integration_unless_it_is_waived() {
    let unlinked = summary_json(1, 0, None, true);
    assert_eq!(
        summary_exit_code(unlinked.clone(), &["--head", HEAD_SHA]).await,
        1
    );
    assert_eq!(
        summary_exit_code(unlinked, &["--head", HEAD_SHA, "--allow-unlinked"]).await,
        0
    );
}

#[tokio::test]
async fn summary_blocks_a_pull_request_that_gained_commits_after_the_review() {
    let clean = summary_json(1, 0, Some("acme/app"), true);
    let stale = ["--head", "0000000000000000000000000000000000000000"];
    assert_eq!(summary_exit_code(clean.clone(), &stale).await, 1);

    // 鮮度を見ないのは明示的に外したときだけ
    assert_eq!(summary_exit_code(clean, &["--no-head-check"]).await, 0);
}

#[tokio::test]
async fn summary_accepts_an_uppercase_head_as_the_same_commit() {
    let clean = summary_json(1, 0, Some("acme/app"), true);
    assert_eq!(
        summary_exit_code(clean, &["--head", &HEAD_SHA.to_uppercase()]).await,
        0
    );
}

/// 設定が無いときでも、直せる引数の誤りを先に報告する。
///
/// 設定不足とゲート不成立はどちらも非 0 で終わるので、順番が入れ替わると
/// 「`--pr` の綴りが違う」ことに気づけないまま設定を疑うことになる。
#[tokio::test]
async fn an_invalid_argument_is_reported_before_missing_configuration() {
    let (_home, context) = harness_without_config();
    let cli = parse(&["task", "review", "list", "--project", "APP", "--pr", "abc"]);

    let err = task_cli::run(cli, &context).await.unwrap_err();
    assert!(
        err.message.contains("--pr must be a positive integer"),
        "{}",
        err.message
    );
}
