//! インストール先リポジトリの選定。
//!
//! このアプリは「1 プロジェクト = 1 リポジトリ」を前提にしているため、
//! インストールが複数のリポジトリにアクセスできる場合は自動で決めずに拒否する。
//! この前提はアプリ固有なので `forge-github` 側には置かない。

use anyhow::anyhow;
use forge_core::Repository;
use forge_github::GithubApp;

/// インストール先リポジトリを選定する（テスト可能な純関数）。
/// 1. リポジトリが 1 件 → そのまま返す
/// 2. preferred_owner と一致するリポジトリが 1 件のみ → それを返す
/// 3. それ以外（複数一致・ゼロ件）→ ユーザーの明示的選択が必要なため `None`
pub fn select_primary_repository<'a>(
    repositories: &'a [Repository],
    preferred_owner: &str,
) -> Option<&'a Repository> {
    if repositories.len() == 1 {
        return repositories.first();
    }
    let mut matches = repositories.iter().filter(|r| r.owner == preferred_owner);
    let first = matches.next()?;
    if matches.next().is_some() {
        return None; // 複数一致
    }
    Some(first)
}

pub async fn fetch_primary_repository(
    app: &GithubApp,
    installation_access_token: &str,
    preferred_owner: &str,
) -> Result<(String, String), anyhow::Error> {
    let repositories = app.list_repositories(installation_access_token).await?;
    let repo = select_primary_repository(&repositories, preferred_owner).ok_or_else(|| {
        anyhow!(
            "installation has access to {} repositories; select one explicitly",
            repositories.len()
        )
    })?;
    Ok((repo.owner.clone(), repo.name.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(owner: &str, name: &str) -> Repository {
        Repository::new(owner, name)
    }

    #[test]
    fn select_primary_repository_returns_none_for_multiple_repos() {
        let repos = vec![
            repo("other-org", "app"),
            repo("myorg", "backend"),
            repo("myorg", "frontend"),
        ];
        assert!(select_primary_repository(&repos, "myorg").is_none());
    }

    #[test]
    fn select_primary_repository_auto_selects_single_repo() {
        let repos = vec![repo("other-org", "app")];
        let chosen = select_primary_repository(&repos, "myorg").unwrap();
        assert_eq!(chosen.to_string(), "other-org/app");
    }

    #[test]
    fn select_primary_repository_prefers_account_owner() {
        let repos = vec![repo("other", "app"), repo("acme", "backend")];
        let chosen = select_primary_repository(&repos, "acme").unwrap();
        assert_eq!(chosen.to_string(), "acme/backend");
    }
}
