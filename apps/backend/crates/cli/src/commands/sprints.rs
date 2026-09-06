//! スプリント。

use entity::sprints::SprintStatus;
use payload::sprints::{BurndownPoint, CompleteSprintRequest, SprintDetail, SprintResponse};
use sea_orm::ActiveEnum;
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
            // 検証せず素通しすると、綴りを外したまま絞り込みが効かない結果が返る
            let status = status.as_deref().map(parse_sprint_status).transpose()?;
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

/// 綴りは entity の `string_value` を正とする。CLI に一覧を写さない。
fn parse_sprint_status(raw: &str) -> Result<String> {
    SprintStatus::try_from_value(&raw.to_ascii_lowercase())
        .map(|status| status.to_value())
        .map_err(|_| {
            CliError::validation(format!(
                "unknown sprint status: {raw} (expected one of {})",
                SprintStatus::values().join(", ")
            ))
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_documented_sprint_statuses() {
        for raw in ["planning", "active", "completed"] {
            assert_eq!(parse_sprint_status(raw).unwrap(), raw);
        }
    }

    #[test]
    fn accepts_a_status_regardless_of_case() {
        assert_eq!(parse_sprint_status("Active").unwrap(), "active");
    }

    /// 素通しさせると、綴りを外したまま絞り込みが効かない結果が返る。
    #[test]
    fn rejects_an_unknown_status_and_lists_the_valid_ones() {
        let err = parse_sprint_status("reviewing").unwrap_err();

        assert_eq!(err.exit_code, 2, "引数の検証エラーは 2");
        assert!(err.message.contains("reviewing"), "{}", err.message);
        for expected in ["planning", "active", "completed"] {
            assert!(err.message.contains(expected), "{}", err.message);
        }
    }
}
