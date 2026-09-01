//! コマンドを実際に走らせるための足場。
//!
//! HTTP はモックサーバーへ向け、設定は一時ディレクトリに置く。実プロセスの
//! 環境変数（`TASK_API_URL` など）は読まないので、手元の設定でテストが揺れない。

// テストバイナリごとに使う足場が違うので、片方では未使用になるものがある。
#![allow(dead_code)]

use clap::Parser;
use task_cli::cli::Cli;
use task_cli::config::{ConfigStore, TaskConfig};
use task_cli::error::Result;
use wiremock::MockServer;

pub const TENANT: &str = "99999999-9999-4999-8999-999999999999";
pub const PROJECT_ID: &str = "11111111-1111-4111-8111-111111111111";
/// 40 桁の小文字 16 進。ゲートが厳密一致で比べるので、短縮 SHA は投入時に弾かれる。
pub const HEAD_SHA: &str = "60cdd7795f94fa4e4148ce996c2efb4c363e3f5e";

pub struct Harness {
    _home: tempfile::TempDir,
    pub server: MockServer,
    context: task_cli::Context,
}

pub async fn harness() -> Harness {
    let server = MockServer::start().await;
    let home = tempfile::tempdir().unwrap();
    let store = ConfigStore::from_home(home.path());
    store
        .save(&TaskConfig {
            api_url: Some(server.uri()),
            token: Some("token-1".into()),
            tenant_id: Some(TENANT.into()),
        })
        .unwrap();

    let context = task_cli::Context::new(ConfigStore::from_home(home.path()), |_| None);
    Harness {
        _home: home,
        server,
        context,
    }
}

/// 設定を置かない足場。検証と設定不足の、どちらが先に報告されるかを確かめる。
pub fn harness_without_config() -> (tempfile::TempDir, task_cli::Context) {
    let home = tempfile::tempdir().unwrap();
    let context = task_cli::Context::new(ConfigStore::from_home(home.path()), |_| None);
    (home, context)
}

pub fn parse(args: &[&str]) -> Cli {
    Cli::try_parse_from(args).expect("the test invoked the CLI with invalid arguments")
}

impl Harness {
    pub async fn run(&self, args: &[&str]) -> Result<i32> {
        let cli =
            Cli::try_parse_from(args).expect("the test invoked the CLI with invalid arguments");
        task_cli::run(cli, &self.context).await
    }

    pub fn store(&self) -> &ConfigStore {
        self.context.store()
    }

    /// 送信前に弾いたことを確かめる（1 本もリクエストが出ていないこと）。
    pub async fn sent_nothing(&self) -> bool {
        self.server
            .received_requests()
            .await
            .is_some_and(|requests| requests.is_empty())
    }
}

pub fn project_json() -> serde_json::Value {
    serde_json::json!({
        "id": PROJECT_ID,
        "name": "App",
        "description": "",
        "tenant_id": TENANT,
        "icon_emoji": null,
        "icon_url": null,
        "key": "APP",
        "is_personal": false,
        "personal_owner_id": null,
    })
}

pub fn status_json(
    id: &str,
    name: &str,
    is_default: bool,
    is_done_state: bool,
    position: i16,
) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "project_id": PROJECT_ID,
        "name": name,
        "color": "#112233",
        "position": position,
        "is_default": is_default,
        "is_done_state": is_done_state,
        "created_at": "2026-01-01T00:00:00Z",
    })
}

pub fn task_detail_json() -> serde_json::Value {
    serde_json::json!({
        "id": "22222222-2222-4222-8222-222222222222",
        "project_id": PROJECT_ID,
        "seq_id": 7,
        "title": "Golden task",
        "description": null,
        "status_id": "33333333-3333-4333-8333-333333333333",
        "priority": "Medium",
        "progress_pct": 0,
        "parent_task_id": null,
        "milestone_id": null,
        "sprint_id": null,
        "soft_deadline": null,
        "hard_deadline": null,
        "estimated_minutes": null,
        "is_archived": false,
        "created_by": null,
        "assignees": [],
        "labels": [],
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z",
        "completed_at": null,
        "deleted_at": null,
        "custom_field_values": [],
    })
}

pub fn summary_json(
    rounds: i32,
    blocking: u64,
    repository: Option<&str>,
    mergeable: bool,
) -> serde_json::Value {
    serde_json::json!({
        "pr_number": 618,
        "rounds": rounds,
        "counts": [],
        "blocking": blocking,
        "latest_head_sha": (rounds > 0).then_some(HEAD_SHA),
        "cached_pr_head_sha": null,
        "pr_head_checked_at": null,
        "owner_override_rejections": 0,
        "repository": repository,
        "mergeable": mergeable,
    })
}

pub fn finding_json() -> serde_json::Value {
    serde_json::json!({
        "id": "44444444-4444-4444-8444-444444444444",
        "review_id": "55555555-5555-4555-8555-555555555555",
        "pr_number": 618,
        "round": 2,
        "severity": "medium",
        "title": "セレクタが複数一致する",
        "body": "説明文にも一致するため",
        "file": "src/App.vue",
        "line": 42,
        "state": "open",
        "deferred_task_id": null,
        "fixed_by": null,
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z",
        "transitions": [],
    })
}

pub fn project_path(suffix: &str) -> String {
    format!("/v1/tenants/{TENANT}/projects/{PROJECT_ID}/{suffix}")
}
