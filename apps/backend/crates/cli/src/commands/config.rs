//! `~/.config/task/config.yaml` の操作。

use serde_json::json;

use crate::cli::ConfigCommand;
use crate::config::{CONFIG_KEYS, ConfigStore};
use crate::error::{CliError, Result};
use crate::output::{OutputOptions, print};

pub fn run(command: ConfigCommand, store: &ConfigStore, output: OutputOptions) -> Result<i32> {
    match command {
        ConfigCommand::List => print(&store.load()?, output),
        ConfigCommand::Get { key } => {
            let key = check_key(&key)?;
            let config = store.load()?;
            let value = config.get(key);
            if output.json {
                print(&json!({ "key": key, "value": value }), output);
            } else {
                println!("{}", value.map(String::as_str).unwrap_or_default());
            }
        }
        ConfigCommand::Set { key, value } => {
            let key = check_key(&key)?;
            let mut config = store.load()?;
            config.set(key, Some(value));
            store.save(&config)?;
            println!("Set {key} in {}", store.path().display());
        }
        ConfigCommand::Unset { key } => {
            let key = check_key(&key)?;
            let mut config = store.load()?;
            config.set(key, None);
            store.save(&config)?;
            println!("Removed {key} from {}", store.path().display());
        }
    }
    Ok(0)
}

fn check_key(key: &str) -> Result<&'static str> {
    CONFIG_KEYS
        .into_iter()
        .find(|allowed| *allowed == key)
        .ok_or_else(|| CliError::validation(format!("Unknown config key: {key}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_key_the_config_file_does_not_have() {
        let err = check_key("api-url").unwrap_err();
        assert!(err.message.contains("Unknown config key: api-url"));
        assert_eq!(err.exit_code, 2);
    }

    #[test]
    fn accepts_every_documented_key() {
        for key in CONFIG_KEYS {
            assert_eq!(check_key(key).unwrap(), key);
        }
    }
}
