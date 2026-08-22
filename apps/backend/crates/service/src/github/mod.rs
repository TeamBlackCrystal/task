//! GitHub App 連携のアプリ固有層。
//!
//! API クライアント自体は `forge-github` クレートにある。ここに置くのは、
//! このアプリの設定からクライアントを組み立てる部分と、インストールフローの
//! state（テナント/プロジェクトに紐づく）、そして「1 プロジェクト = 1 リポジトリ」
//! という前提に基づくリポジトリ選定。

pub mod client;
pub mod import_lock;
pub mod install_state;
pub mod issues;
pub mod repositories;
pub mod sync;

pub use client::github_app;
pub use import_lock::{IMPORT_LOCK_TTL_SECS, release_import_slot, try_acquire_import_slot};
pub use install_state::{
    GithubOAuthStatePayload, TTL_SECS, consume_state, new_state_token, store_state,
};
pub use repositories::{fetch_primary_repository, select_primary_repository};
pub use sync::{apply_issue, import_project, push_task};
