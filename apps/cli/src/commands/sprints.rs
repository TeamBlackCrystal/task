//! スプリント。

use payload::sprints::{BurndownPoint, CompleteSprintRequest, SprintDetail, SprintResponse};
use serde::Serialize;
use uuid::Uuid;

use crate::Context;
use crate::api::ApiClient;
use crate::cli::SprintsCommand;
use crate::error::{CliError, Result};
use crate::output::{OutputOptions, print};
use crate::resolve::{is_uuid, resolve_project};

pub async fn run(context: &Context, command: SprintsCommand, output: OutputOptions) -> Result<i32> {
    let api = &context.connect()?;
    match command {
        SprintsCommand::List { project, status } => {
            let project = resolve_project(api, &project).await?;
            let query = status
                .map(|status| vec![("status", status)])
                .unwrap_or_default();
            let sprints: Vec<SprintResponse> = api
                .get(&borrow(&sprints_path(api, project.id)), &query)
                .await?;
            print(&sprints, output);
        }
        SprintsCommand::Show { id, project } => {
            let project = resolve_project(api, &project).await?;
            let detail: SprintDetail = api
                .get(&borrow(&sprint_path(api, project.id, &id)), &[])
                .await?;
            print(&detail, output);
        }
        SprintsCommand::Start { id, project } => {
            let project = resolve_project(api, &project).await?;
            let mut segments = sprint_path(api, project.id, &id);
            segments.push("start".into());
            let sprint: SprintResponse = api.post(&borrow(&segments), &()).await?;
            print(&sprint, output);
        }
        SprintsCommand::Complete {
            id,
            project,
            backlog,
        } => {
            let project = resolve_project(api, &project).await?;
            let mut segments = sprint_path(api, project.id, &id);
            segments.push("complete".into());
            let body = CompleteSprintRequest {
                move_incomplete_to_sprint_id: None,
                move_incomplete_to_backlog: backlog,
            };
            let sprint: SprintResponse = api.post(&borrow(&segments), &body).await?;
            print(&sprint, output);
        }
        SprintsCommand::Burndown { id, project } => {
            let project = resolve_project(api, &project).await?;
            let sprint_id = resolve_sprint_id(api, project.id, &id).await?;
            let detail: SprintDetail = api
                .get(&borrow(&sprint_path(api, project.id, &sprint_id)), &[])
                .await?;
            if output.json {
                print(
                    &Burndown {
                        sprint: &detail.sprint,
                        burndown: &detail.burndown,
                    },
                    output,
                );
            } else {
                print(&detail.burndown, output);
            }
        }
    }
    Ok(0)
}

/// `--json` のときはどのスプリントの数字かが要る。
#[derive(Serialize)]
struct Burndown<'a> {
    sprint: &'a SprintResponse,
    burndown: &'a [BurndownPoint],
}

async fn resolve_sprint_id(api: &ApiClient, project_id: Uuid, id_or_name: &str) -> Result<String> {
    if is_uuid(id_or_name) {
        return Ok(id_or_name.to_string());
    }
    let sprints: Vec<SprintResponse> = api
        .get(&borrow(&sprints_path(api, project_id)), &[])
        .await?;
    sprints
        .into_iter()
        .find(|sprint| sprint.name.eq_ignore_ascii_case(id_or_name))
        .map(|sprint| sprint.id.to_string())
        .ok_or_else(|| CliError::not_found(format!("Sprint not found: {id_or_name}")))
}

fn sprints_path(api: &ApiClient, project_id: Uuid) -> Vec<String> {
    vec![
        "v1".into(),
        "tenants".into(),
        api.tenant_id().into(),
        "projects".into(),
        project_id.to_string(),
        "sprints".into(),
    ]
}

fn sprint_path(api: &ApiClient, project_id: Uuid, sprint_id: &str) -> Vec<String> {
    let mut segments = sprints_path(api, project_id);
    segments.push(sprint_id.to_string());
    segments
}

fn borrow(segments: &[String]) -> Vec<&str> {
    segments.iter().map(String::as_str).collect()
}
