//! GitHub App 連携のアプリ固有層。
//!
//! API クライアント自体は `forge-github` クレートにある。ここに置くのは、
//! このアプリの設定からクライアントを組み立てる部分と、インストールフローの
//! state（テナント/プロジェクトに紐づく）、そして「1 プロジェクト = 1 リポジトリ」
//! という前提に基づくリポジトリ選定。

pub mod client;
pub mod install_state;
pub mod issues;
pub mod repositories;
pub mod sync;

pub use client::github_app;
pub use install_state::{
    GithubOAuthStatePayload, RepoSelectPayload, TTL_SECS, consume_state,
    delete_pending_installation_if, new_state_token, peek_pending_installation, peek_select_token,
    store_pending_installation, store_select_token, store_state,
};
pub use repositories::{contains_repository, select_primary_repository};
pub use sync::{apply_issue, import_project, push_task};
