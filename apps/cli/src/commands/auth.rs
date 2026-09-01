//! 認証。トークンの保存だけは API を呼ばずに済ませる。

use std::io::{IsTerminal, Read};

use payload::users::UserResponse;

use crate::Context;
use crate::cli::AuthCommand;
use crate::error::{CliError, Result};
use crate::output::{OutputOptions, print};

pub async fn run(command: AuthCommand, context: &Context, output: OutputOptions) -> Result<i32> {
    let store = context.store();
    match command {
        AuthCommand::Whoami => {
            let api = context.connect()?;
            let user: UserResponse = api.get(&["v1", "auth", "me"], &[]).await?;
            print(&user, output);
        }
        AuthCommand::Token { token } => {
            let token = match token
                .map(|value| value.trim().to_string())
                .filter(|v| !v.is_empty())
            {
                Some(token) => token,
                None => read_stdin()?,
            };
            if token.is_empty() {
                return Err(CliError::new("Token is required"));
            }
            let mut config = store.load()?;
            config.token = Some(token);
            store.save(&config)?;
            println!("Token saved to {}", store.path().display());
        }
        AuthCommand::Logout => {
            let mut config = store.load()?;
            config.token = None;
            store.save(&config)?;
            println!("Token removed from {}", store.path().display());
        }
    }
    Ok(0)
}

/// 端末から実行されたときは待ち受けない（貼り付け待ちで固まって見えるのを避ける）。
fn read_stdin() -> Result<String> {
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Ok(String::new());
    }
    let mut buffer = String::new();
    stdin
        .lock()
        .read_to_string(&mut buffer)
        .map_err(|err| CliError::new(format!("Cannot read the token from stdin: {err}")))?;
    Ok(buffer.trim().to_string())
}
