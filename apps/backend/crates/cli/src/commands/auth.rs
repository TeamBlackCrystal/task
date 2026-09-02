//! 認証。トークンの保存だけは API を呼ばずに済ませる。

use payload::users::UserResponse;

use crate::Context;
use crate::cli::AuthCommand;
use crate::error::{CliError, Result};
use crate::output::{OutputOptions, print};
use crate::text_input::read_stdin;

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
                None => read_stdin("token")?.unwrap_or_default().trim().to_string(),
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
