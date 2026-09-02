//! プロジェクト。

use serde::Serialize;

use crate::Context;
use crate::cli::ProjectsCommand;
use crate::error::Result;
use crate::output::{OutputOptions, print};
use crate::resolve::{list_projects, list_statuses, resolve_project};

/// 人間向けの一覧はキーで包んで出す（TypeScript 版と同じ形）。
#[derive(Serialize)]
struct ProjectListing<T> {
    projects: T,
}

#[derive(Serialize)]
struct StatusListing<T> {
    statuses: T,
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
        // `--status` に何を渡せるかは、プロジェクトごとに違ううえ画面を見ないと分からない。
        // CLI だけで完結できるよう並び順（position 昇順）のまま出す
        ProjectsCommand::Statuses { project } => {
            let project = resolve_project(api, &project).await?;
            let mut statuses = list_statuses(api, project.id).await?;
            statuses.sort_by_key(|status| status.position);
            if output.json {
                print(&statuses, output);
            } else {
                print(
                    &StatusListing {
                        statuses: &statuses,
                    },
                    output,
                );
            }
        }
        ProjectsCommand::Show { key } => print(&resolve_project(api, &key).await?, output),
    }
    Ok(0)
}
