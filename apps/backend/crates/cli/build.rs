//! `task --version` が名乗る版を決める。
//!
//! リリースはリポジトリ共通の `v*` タグで行うので、CLI だけが独自の版を持つと
//! タグと `--version` がずれる。タグから `TASK_CLI_VERSION` を渡してもらい、
//! 無ければクレートの版を使う（手元のビルドはこちら）。

fn main() {
    let version = std::env::var("TASK_CLI_VERSION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());

    println!("cargo:rustc-env=TASK_CLI_VERSION={version}");
    println!("cargo:rerun-if-env-changed=TASK_CLI_VERSION");
}
