//! Task management CLI。
//!
//! API の型（`payload`）と検証の規則（`entity` / `common`）は backend と共有する。
//! CLI 側に写しを置くと、API の変更に追いつかないまま黙って食い違う（#647）。

pub mod api;
pub mod cli;
pub mod commands;
pub mod config;
pub mod error;
pub mod output;
pub mod resolve;
pub mod text_input;

use api::ApiClient;
use cli::{Cli, Command};
use config::{ConfigStore, RuntimeConfig, resolve_runtime_with};
use error::Result;
use output::OutputOptions;

/// 環境変数の読み取り。テストは実プロセスの環境を見ない実装を差し込む。
type EnvLookup = Box<dyn Fn(&str) -> Option<String>>;

/// 設定ファイルと環境変数の出どころ。
///
/// テストが実プロセスの環境変数に左右されないよう、読み取り先を値で持つ。
pub struct Context {
    store: ConfigStore,
    env: EnvLookup,
}

impl Context {
    pub fn from_process() -> Result<Self> {
        Ok(Self::new(ConfigStore::discover()?, |key| {
            std::env::var(key).ok()
        }))
    }

    pub fn new(store: ConfigStore, env: impl Fn(&str) -> Option<String> + 'static) -> Self {
        Self {
            store,
            env: Box::new(env),
        }
    }

    pub fn store(&self) -> &ConfigStore {
        &self.store
    }

    fn runtime(&self) -> Result<RuntimeConfig> {
        resolve_runtime_with(&self.store, |key| (self.env)(key))
    }

    /// API クライアントを作る。設定の不足を、引数の検証より先に報告しないよう
    /// 呼ぶのは各コマンドが実際に送る直前にする。
    pub(crate) fn connect(&self) -> Result<ApiClient> {
        ApiClient::new(self.runtime()?)
    }
}

/// プロセスの終了コードを返す。0 以外はゲート不成立か失敗。
pub async fn run(cli: Cli, context: &Context) -> Result<i32> {
    let output = OutputOptions::new(cli.json);

    // 設定を要らないコマンド（`config` と手元のトークン操作）を、設定不足で落とさない。
    // 落とすと、トークンを保存する前に何もできなくなる
    match cli.command {
        Command::Config { command } => commands::config::run(command, context.store(), output),
        Command::Auth { command } => commands::auth::run(command, context, output).await,
        Command::Projects { command } => commands::projects::run(context, command, output).await,
        Command::Tasks { command } => commands::tasks::run(context, *command, output).await,
        Command::My { command } => commands::my::run(context, command, output).await,
        Command::Sprints { command } => commands::sprints::run(context, command, output).await,
        Command::Review { command } => commands::reviews::run(context, command, output).await,
    }
}
