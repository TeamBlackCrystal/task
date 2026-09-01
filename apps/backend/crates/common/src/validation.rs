use std::sync::LazyLock;

use regex::Regex;

pub static COLOR_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^#[0-9A-Fa-f]{6}$").unwrap());

/// commit SHA（40 桁の小文字 16 進）。
///
/// 短縮 SHA を受け取ると、マージ可否のゲートが `latest_head_sha` を厳密一致で
/// 比べるため、そのラウンドは以後どれだけ指摘を解消しても通らなくなる。しかも
/// 「同じ commit に見えるのに再レビューを要求される」形で表示されるので、
/// 原因が投入時の書き方にあることを画面から辿れない。書き込む側で弾く。
pub static COMMIT_SHA_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9a-f]{40}$").unwrap());
