//! アプリ設定から GitHub App クライアントを組み立てる。

use common::settings::GithubAppSettings;
use forge_github::{GithubApp, GithubAppCredentials, GithubAppOAuthCredentials};
use reqwest::Client;

/// GitHub API に送る User-Agent。
const USER_AGENT: &str = "task-backend";

/// GitHub REST API の既定のベース URL。
const DEFAULT_API_BASE: &str = "https://api.github.com";

/// GitHub のベース URL を差し替えてよい宛先。
///
/// ここを差し替えると GitHub の資格情報を載せたリクエストの宛先が変わるので、
/// 統合テストのモックサーバー（ループバック）以外は受け付けない。
/// env を 1 本間違えただけでシークレットが外へ出るのを防ぐ。
/// IPv6 は `Url::host_str` が角括弧付きで返すため、両方の表記を持つ。
const LOOPBACK_HOSTS: [&str; 4] = ["127.0.0.1", "localhost", "::1", "[::1]"];

fn loopback_base(base: &str) -> Option<String> {
    let base = base.trim();
    if base.is_empty() {
        return None;
    }
    let host = reqwest::Url::parse(base).ok()?.host_str()?.to_owned();
    if LOOPBACK_HOSTS.contains(&host.as_str()) {
        Some(base.to_owned())
    } else {
        None
    }
}

/// 環境変数に設定された差し替え先を、ループバック宛てのときだけ返す。
pub(super) fn loopback_base_override(var: &str) -> Option<String> {
    let base = std::env::var(var).ok()?;
    if base.trim().is_empty() {
        return None;
    }
    match loopback_base(&base) {
        Some(base) => Some(base),
        None => {
            let host = reqwest::Url::parse(base.trim())
                .ok()
                .and_then(|url| url.host_str().map(str::to_owned));
            tracing::warn!(
                variable = var,
                host = host.as_deref().unwrap_or("<invalid>"),
                "ignoring non-loopback GitHub base URL"
            );
            None
        }
    }
}

/// GitHub REST API のベース URL。
///
/// 既定は `https://api.github.com`。`GITHUB_API_BASE_URL` は**ループバック宛てのときだけ**
/// 採る（統合テストがモックサーバーを向けるための口で、それ以外の用途は無い）。
/// ここを素の環境変数で読むと、この URL へ載る installation token が任意のホストへ渡る。
pub(super) fn api_base() -> String {
    loopback_base_override("GITHUB_API_BASE_URL")
        .map(|base| base.trim_end_matches('/').to_string())
        .unwrap_or_else(|| DEFAULT_API_BASE.to_string())
}

/// 設定から [`GithubApp`] を作る。
///
/// `GITHUB_API_BASE_URL` / `GITHUB_OAUTH_BASE_URL` が設定されていればベース URL を
/// 差し替える（統合テストがモックサーバーを向けるために使う）。API とユーザー認可は
/// GitHub でもホストが分かれている（`api.github.com` と `github.com`）ため、
/// 差し替え先も別々に持つ。
pub fn github_app(http: &Client, settings: &GithubAppSettings) -> GithubApp {
    let app = GithubApp::new(
        http.clone(),
        GithubAppCredentials::new(
            settings.github_app_id.clone(),
            settings.github_app_private_key.clone(),
        ),
    )
    .with_user_agent(USER_AGENT)
    .with_oauth_credentials(GithubAppOAuthCredentials::new(
        settings.github_app_client_id.clone(),
        settings.github_app_client_secret.clone(),
    ));

    let app = match loopback_base_override("GITHUB_API_BASE_URL") {
        Some(base) => app.with_api_base(base),
        None => app,
    };
    match loopback_base_override("GITHUB_OAUTH_BASE_URL") {
        Some(base) => app.with_oauth_base(base),
        None => app,
    }
}

#[cfg(test)]
mod tests {
    use super::{api_base, loopback_base};

    /// `api_base` は非ループバックの上書きを無視して既定へ落ちる。
    ///
    /// この URL には GitHub App の installation token が `Authorization: Bearer` で
    /// 載るので、素の環境変数で読むとその資格情報が任意のホストへ渡る。
    /// `loopback_base` 単体のテストでは、呼び出し側が helper を通していることまでは
    /// 固定できない（実際、要約コメントの経路だけ素読みしていた）。
    ///
    /// 環境変数を書き換えるが、このクレートでこの変数を読むのは `api_base` と
    /// `github_app` だけで、どちらも他のテストからは呼ばれない。
    #[test]
    fn api_base_ignores_non_loopback_overrides() {
        // SAFETY: この変数を読むテストは他に無く、書き換えが競合しない。
        unsafe { std::env::set_var("GITHUB_API_BASE_URL", "https://evil.example.com") };
        assert_eq!(api_base(), "https://api.github.com");

        // 対照: ループバックなら差し替わる（統合テストのモックが向く先）
        unsafe { std::env::set_var("GITHUB_API_BASE_URL", "http://127.0.0.1:8080/") };
        assert_eq!(api_base(), "http://127.0.0.1:8080");

        // SAFETY: 上と同じ。
        unsafe { std::env::remove_var("GITHUB_API_BASE_URL") };
        assert_eq!(api_base(), "https://api.github.com");
    }

    #[test]
    fn accepts_loopback_github_base_urls() {
        for base in [
            "http://127.0.0.1:8080",
            "http://localhost:8080/",
            "http://[::1]:8080",
        ] {
            assert_eq!(loopback_base(base).as_deref(), Some(base));
        }
    }

    /// GitHub API を叩く側が、ベース URL を環境変数から素読みしていない。
    ///
    /// [`api_base_ignores_non_loopback_overrides`] は helper の挙動しか固定できず、
    /// 呼び出し側が helper を迂回して `std::env::var` を読む形に戻ると素通りする
    /// （要約コメントの経路が実際にそうなっていた）。
    ///
    /// 対象は手で並べず `github/` を走査する。防ぎたいのは「新しいモジュールで
    /// helper を通し忘れる」形なので、一覧を手書きにすると増えた経路を拾えず、
    /// まさにその再発を見逃す。
    #[test]
    fn github_api_callers_do_not_read_the_base_url_directly() {
        let var = "GITHUB_API_BASE_URL";
        let needle = format!("env::var(\"{var}\")");
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/github");

        let mut checked = 0;
        for entry in std::fs::read_dir(dir).expect("read github module dir") {
            let path = entry.expect("dir entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .expect("file name")
                .to_owned();
            // client.rs だけが読んでよい（ここが唯一の入口）
            if name == "client.rs" {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("read module");
            assert!(
                !source.contains(&needle),
                "{name} は {var} を素読みしている。client::api_base() を使う\
                 （この URL には installation token が載るので、ループバック制限を外せない）"
            );
            checked += 1;
        }
        assert!(checked > 0, "走査対象が 0 件（パスがずれている: {dir}）");
    }

    #[test]
    fn rejects_non_loopback_github_base_urls() {
        for base in [
            "https://example.com",
            "http://192.0.2.1:8080",
            "not a url",
            "",
            "   ",
        ] {
            assert_eq!(loopback_base(base), None, "base URL: {base:?}");
        }
    }
}
