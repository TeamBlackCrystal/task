//! GitHub App 連携のアプリ固有層。
//!
//! API クライアント自体は `forge-github` クレートにある。ここに置くのは、
//! このアプリの設定からクライアントを組み立てる部分と、インストールフローの
//! state（テナント/プロジェクトに紐づく）、そして「1 プロジェクト = 1 リポジトリ」
//! という前提に基づくリポジトリ選定。

pub mod client;
pub mod install_state;
pub mod repositories;

pub use client::github_app;
pub use install_state::{
    GithubOAuthStatePayload, TTL_SECS, consume_state, new_state_token, store_state,
};
pub use repositories::{fetch_primary_repository, select_primary_repository};
