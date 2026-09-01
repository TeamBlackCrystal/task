use clap::Parser;

use task_cli::Context;
use task_cli::cli::Cli;

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    let context = match Context::from_process() {
        Ok(context) => context,
        Err(err) => return fail(err),
    };
    match task_cli::run(cli, &context).await {
        Ok(code) => std::process::ExitCode::from(code as u8),
        Err(err) => fail(err),
    }
}

fn fail(err: task_cli::error::CliError) -> std::process::ExitCode {
    eprintln!("{}", err.message);
    std::process::ExitCode::from(err.exit_code as u8)
}
