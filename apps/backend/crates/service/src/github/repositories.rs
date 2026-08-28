//! インストール先リポジトリの選定。
//!
//! このアプリは「1 プロジェクト = 1 リポジトリ」を前提にしているため、
//! インストールが複数のリポジトリにアクセスできる場合は自動で決めず、
//! ユーザーに選ばせる（選択 UI は callback が発行する選択トークン経由）。
//! この前提はアプリ固有なので `forge-github` 側には置かない。

use forge_core::Repository;

/// 未選択のまま自動で連携してよいリポジトリを返す。
/// アクセスできるリポジトリが 1 件のときだけ自動選択し、複数なら `None`（要選択）。
pub fn select_primary_repository(repositories: &[Repository]) -> Option<&Repository> {
    match repositories {
        [only] => Some(only),
        _ => None,
    }
}

/// 選択されたリポジトリが、その installation の可視範囲に含まれるか検証する。
pub fn contains_repository(repositories: &[Repository], owner: &str, name: &str) -> bool {
    repositories
        .iter()
        .any(|r| r.owner == owner && r.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(owner: &str, name: &str) -> Repository {
        Repository::new(owner, name)
    }

    #[test]
    fn select_primary_repository_returns_none_for_multiple_repos() {
        let repos = vec![repo("myorg", "backend"), repo("myorg", "frontend")];
        assert!(select_primary_repository(&repos).is_none());
    }

    #[test]
    fn select_primary_repository_returns_none_for_empty() {
        assert!(select_primary_repository(&[]).is_none());
    }

    #[test]
    fn select_primary_repository_auto_selects_single_repo() {
        let repos = vec![repo("other-org", "app")];
        let chosen = select_primary_repository(&repos).unwrap();
        assert_eq!(chosen.to_string(), "other-org/app");
    }

    #[test]
    fn contains_repository_matches_owner_and_name() {
        let repos = vec![repo("acme", "backend"), repo("acme", "frontend")];
        assert!(contains_repository(&repos, "acme", "frontend"));
        assert!(!contains_repository(&repos, "acme", "unknown"));
        assert!(!contains_repository(&repos, "other", "backend"));
    }
}
