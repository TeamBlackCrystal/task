//! `~/.config/task/config.yaml` の読み書きと、実行時設定の解決。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{CliError, Result};

/// `config get` / `set` / `unset` が触れるキー。
pub const CONFIG_KEYS: [&str; 3] = ["api_url", "token", "tenant_id"];

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
}

impl TaskConfig {
    pub fn get(&self, key: &str) -> Option<&String> {
        match key {
            "api_url" => self.api_url.as_ref(),
            "token" => self.token.as_ref(),
            "tenant_id" => self.tenant_id.as_ref(),
            _ => None,
        }
    }

    pub fn set(&mut self, key: &str, value: Option<String>) {
        match key {
            "api_url" => self.api_url = value,
            "token" => self.token = value,
            "tenant_id" => self.tenant_id = value,
            _ => {}
        }
    }
}

/// 設定ファイルの置き場所。テストが一時ディレクトリを指せるよう値で持つ。
#[derive(Debug, Clone)]
pub struct ConfigStore {
    dir: PathBuf,
}

impl ConfigStore {
    pub fn from_home(home: &Path) -> Self {
        Self {
            dir: home.join(".config").join("task"),
        }
    }

    pub fn discover() -> Result<Self> {
        let home = std::env::var("HOME").map_err(|_| {
            CliError::validation("Cannot locate the home directory (HOME is unset)")
        })?;
        Ok(Self::from_home(Path::new(&home)))
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join("config.yaml")
    }

    pub fn load(&self) -> Result<TaskConfig> {
        let path = self.path();
        if !path.exists() {
            return Ok(TaskConfig::default());
        }
        let raw = std::fs::read_to_string(&path)
            .map_err(|err| CliError::new(format!("Cannot read {}: {err}", path.display())))?;
        // 空ファイルは YAML の null になる。設定が無い状態として扱う。
        serde_yaml_ng::from_str::<Option<TaskConfig>>(&raw)
            .map(Option::unwrap_or_default)
            .map_err(|err| CliError::validation(format!("Cannot parse {}: {err}", path.display())))
    }

    pub fn save(&self, config: &TaskConfig) -> Result<()> {
        std::fs::create_dir_all(&self.dir)
            .map_err(|err| CliError::new(format!("Cannot create {}: {err}", self.dir.display())))?;
        let path = self.path();
        let body = serde_yaml_ng::to_string(config)
            .map_err(|err| CliError::new(format!("Cannot serialize config: {err}")))?;
        std::fs::write(&path, body)
            .map_err(|err| CliError::new(format!("Cannot write {}: {err}", path.display())))?;
        restrict_permissions(&path)
    }
}

/// トークンを含むので所有者だけが読めるようにする。
///
/// 書き込み時のモード指定は新規作成にしか効かないため、既存ファイルの更新でも
/// 落ちないよう毎回付け直す。
#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|err| CliError::new(format!("Cannot chmod {}: {err}", path.display())))
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

/// API を呼ぶのに必要な 3 点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeConfig {
    pub api_url: String,
    pub token: String,
    pub tenant_id: String,
}

/// 環境変数を設定ファイルより優先して解決する。
pub fn resolve_runtime_with(
    store: &ConfigStore,
    env: impl Fn(&str) -> Option<String>,
) -> Result<RuntimeConfig> {
    let file = store.load()?;
    let api_url = env("TASK_API_URL").or(file.api_url);
    let token = env("TASK_TOKEN").or(file.token);
    let tenant_id = env("TASK_TENANT").or(file.tenant_id);

    let missing: Vec<&str> = [
        api_url.is_none().then_some("api_url (TASK_API_URL)"),
        token.is_none().then_some("token (TASK_TOKEN)"),
        tenant_id.is_none().then_some("tenant_id (TASK_TENANT)"),
    ]
    .into_iter()
    .flatten()
    .collect();

    if !missing.is_empty() {
        return Err(CliError::validation(format!(
            "Missing required configuration: {}. Set env vars or {}.",
            missing.join(", "),
            store.path().display(),
        )));
    }

    Ok(RuntimeConfig {
        // 末尾の `/` を落とさないとパスを繋いだ URL が `//v1/...` になる
        api_url: api_url.unwrap().trim_end_matches('/').to_string(),
        token: token.unwrap(),
        tenant_id: tenant_id.unwrap(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, ConfigStore) {
        let home = tempfile::tempdir().unwrap();
        let store = ConfigStore::from_home(home.path());
        (home, store)
    }

    #[test]
    fn reads_and_writes_only_below_the_given_home() {
        let (home, store) = store();
        assert_eq!(store.path(), home.path().join(".config/task/config.yaml"));
        assert_eq!(store.load().unwrap(), TaskConfig::default());

        store
            .save(&TaskConfig {
                api_url: Some("https://task.invalid/".into()),
                token: Some("secret".into()),
                tenant_id: Some("tenant-1".into()),
            })
            .unwrap();

        assert_eq!(store.load().unwrap().token.as_deref(), Some("secret"));
    }

    #[cfg(unix)]
    #[test]
    fn keeps_the_token_file_readable_only_by_its_owner_across_rewrites() {
        use std::os::unix::fs::PermissionsExt;

        let (_home, store) = store();
        store
            .save(&TaskConfig {
                token: Some("a".into()),
                ..Default::default()
            })
            .unwrap();
        // 上書き時にモード指定が効かない経路を踏ませる（2 回目でも 0600 のままか）
        store
            .save(&TaskConfig {
                token: Some("b".into()),
                ..Default::default()
            })
            .unwrap();

        let mode = std::fs::metadata(store.path())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn treats_an_empty_config_file_as_no_configuration() {
        let (_home, store) = store();
        std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        std::fs::write(store.path(), "").unwrap();

        assert_eq!(store.load().unwrap(), TaskConfig::default());
    }

    #[test]
    fn prefers_environment_values_and_removes_a_trailing_api_slash() {
        let (_home, store) = store();
        store
            .save(&TaskConfig {
                api_url: Some("https://file.invalid".into()),
                token: Some("file-token".into()),
                tenant_id: Some("file-tenant".into()),
            })
            .unwrap();

        let resolved = resolve_runtime_with(&store, |key| match key {
            "TASK_API_URL" => Some("https://api.invalid/".into()),
            "TASK_TOKEN" => Some("env-token".into()),
            "TASK_TENANT" => Some("env-tenant".into()),
            _ => None,
        })
        .unwrap();

        assert_eq!(
            resolved,
            RuntimeConfig {
                api_url: "https://api.invalid".into(),
                token: "env-token".into(),
                tenant_id: "env-tenant".into(),
            }
        );
    }

    #[test]
    fn falls_back_to_the_file_when_only_part_of_the_environment_is_set() {
        let (_home, store) = store();
        store
            .save(&TaskConfig {
                api_url: Some("https://file.invalid".into()),
                token: Some("file-token".into()),
                tenant_id: Some("file-tenant".into()),
            })
            .unwrap();

        let resolved = resolve_runtime_with(&store, |key| {
            (key == "TASK_TOKEN").then(|| "env-token".to_string())
        })
        .unwrap();

        assert_eq!(resolved.api_url, "https://file.invalid");
        assert_eq!(resolved.token, "env-token");
        assert_eq!(resolved.tenant_id, "file-tenant");
    }

    #[test]
    fn names_every_missing_setting_and_exits_two() {
        let (_home, store) = store();
        let err = resolve_runtime_with(&store, |_| None).unwrap_err();

        assert_eq!(err.exit_code, 2);
        for expected in [
            "api_url (TASK_API_URL)",
            "token (TASK_TOKEN)",
            "tenant_id (TASK_TENANT)",
        ] {
            assert!(err.message.contains(expected), "{}", err.message);
        }
    }

    #[test]
    fn drops_a_key_from_the_file_when_it_is_unset() {
        let (_home, store) = store();
        let mut config = TaskConfig {
            api_url: Some("https://task.invalid".into()),
            token: Some("secret".into()),
            tenant_id: None,
        };
        store.save(&config).unwrap();

        config.set("token", None);
        store.save(&config).unwrap();

        let raw = std::fs::read_to_string(store.path()).unwrap();
        assert!(!raw.contains("token"), "{raw}");
        assert_eq!(store.load().unwrap().token, None);
    }
}
