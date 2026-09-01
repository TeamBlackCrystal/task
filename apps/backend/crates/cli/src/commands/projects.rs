//! プロジェクト。

use serde::Serialize;

use crate::Context;
use crate::cli::ProjectsCommand;
use crate::error::Result;
use crate::output::{OutputOptions, print};
use crate::resolve::{list_projects, resolve_project};

/// 人間向けの一覧はキーで包んで出す（TypeScript 版と同じ形）。
#[derive(Serialize)]
struct ProjectListing<T> {
    projects: T,
}

pub async fn run(
    context: &Context,
    command: ProjectsCommand,
    output: OutputOptions,
) -> Result<i32> {
    let api = &context.connect()?;
    match command {
        ProjectsCommand::List => {
            let projects = list_projects(api).await?;
            if output.json {
                print(&projects, output);
            } else {
                print(
                    &ProjectListing {
                        projects: &projects,
                    },
                    output,
                );
            }
        }
        ProjectsCommand::Show { key } => print(&resolve_project(api, &key).await?, output),
    }
    Ok(0)
}
