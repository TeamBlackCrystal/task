//! タスク。

use entity::tasks::TaskPriority;
use payload::projects::ProjectResponse;
use payload::task_comments::{CreateCommentRequest, TaskCommentResponse};
use payload::tasks::{CreateTaskRequest, TaskDetailResponse, TaskListResponse, UpdateTaskRequest};
use sea_orm::ActiveEnum;
use serde_json::json;
use uuid::Uuid;

use crate::Context;
use crate::api::ApiClient;
use crate::cli::TasksCommand;
use crate::error::{CliError, Result};
use crate::output::{OutputOptions, print};
use crate::resolve::{
    TaskRef, default_status_id, find_done_status_id, parse_task_ref, resolve_project,
    resolve_status_id,
};

pub async fn run(context: &Context, command: TasksCommand, output: OutputOptions) -> Result<i32> {
    match command {
        TasksCommand::List { project, priority } => {
            // 一覧の絞り込みはクエリ文字列で受ける（`priority=medium`）。値の綴りは
            // 作成・更新の本文と同じなので、ここでも送信前に確かめる
            let query = match priority {
                Some(priority) => vec![("priority", parse_priority(&priority)?.to_value())],
                None => vec![],
            };
            let api = &context.connect()?;
            let project = resolve_project(api, &project).await?;
            let tasks: TaskListResponse = api
                .get(&borrow(&tasks_path(api, project.id)), &query)
                .await?;
            print(&tasks, output);
        }
        TasksCommand::Create {
            project,
            title,
            priority,
            status,
        } => {
            let priority = priority.as_deref().map(parse_priority).transpose()?;
            let api = &context.connect()?;
            let project = resolve_project(api, &project).await?;
            let status_id = match status {
                Some(status) => resolve_status_id(api, project.id, &status).await?,
                // 作成 API は status_id を必須で受ける。省略時に送らないと必ず 400 になる
                None => default_status_id(api, project.id).await?,
            };
            let body = CreateTaskRequest {
                title,
                description: None,
                status_id,
                priority,
                progress_pct: None,
                parent_task_id: None,
                milestone_id: None,
                sprint_id: None,
                soft_deadline: None,
                hard_deadline: None,
                estimated_minutes: None,
                assignees: vec![],
                label_ids: vec![],
                custom_field_values: vec![],
            };
            let created: TaskDetailResponse = api
                .post(&borrow(&tasks_path(api, project.id)), &body)
                .await?;
            print(&created, output);
        }
        TasksCommand::Show { task_ref, project } => {
            let target = check_task_target(&task_ref, project.as_deref())?;
            let api = &context.connect()?;
            let (project, task_id) = resolve_task_target(api, target).await?;
            let task: TaskDetailResponse = api
                .get(&borrow(&task_path(api, project.id, &task_id)), &[])
                .await?;
            print(&task, output);
        }
        TasksCommand::Update {
            task_ref,
            project,
            title,
            status,
            priority,
        } => {
            let priority = priority.as_deref().map(parse_priority).transpose()?;
            let target = check_task_target(&task_ref, project.as_deref())?;
            let api = &context.connect()?;
            let (project, task_id) = resolve_task_target(api, target).await?;
            let status_id = match status {
                Some(status) => Some(resolve_status_id(api, project.id, &status).await?),
                None => None,
            };
            let body = update_request(UpdateFields {
                title,
                status_id,
                priority,
            });
            let updated: TaskDetailResponse = api
                .put(&borrow(&task_path(api, project.id, &task_id)), &body)
                .await?;
            print(&updated, output);
        }
        TasksCommand::Complete { task_ref, project } => {
            let target = check_task_target(&task_ref, project.as_deref())?;
            let api = &context.connect()?;
            let (project, task_id) = resolve_task_target(api, target).await?;
            let status_id = find_done_status_id(api, project.id).await?;
            let body = done_request(status_id);
            let updated: TaskDetailResponse = api
                .put(&borrow(&task_path(api, project.id, &task_id)), &body)
                .await?;
            print(&updated, output);
        }
        TasksCommand::Comment {
            task_ref,
            body,
            project,
        } => {
            let target = check_task_target(&task_ref, project.as_deref())?;
            let api = &context.connect()?;
            let (project, task_id) = resolve_task_target(api, target).await?;
            let mut segments = task_path(api, project.id, &task_id);
            segments.push("comments".into());
            let comment: TaskCommentResponse = api
                .post(
                    &borrow(&segments),
                    &CreateCommentRequest {
                        body,
                        parent_comment_id: None,
                    },
                )
                .await?;
            print(&comment, output);
        }
        TasksCommand::Delete { task_ref, project } => {
            let target = check_task_target(&task_ref, project.as_deref())?;
            let api = &context.connect()?;
            let (project, task_id) = resolve_task_target(api, target).await?;
            api.delete(&borrow(&task_path(api, project.id, &task_id)))
                .await?;
            if output.json {
                print(&json!({ "deleted": task_id }), output);
            } else {
                println!("Deleted {task_id}");
            }
        }
    }
    Ok(0)
}

/// 状態だけを完了へ動かす更新本文（`tasks complete` と `my complete` が共有する）。
pub(crate) fn done_request(status_id: Uuid) -> UpdateTaskRequest {
    update_request(UpdateFields {
        title: None,
        status_id: Some(status_id),
        priority: None,
    })
}

struct UpdateFields {
    title: Option<String>,
    status_id: Option<Uuid>,
    priority: Option<TaskPriority>,
}

/// 更新は「渡されたものだけ変える」。`clear_*` は明示的な解除用なので常に false。
fn update_request(fields: UpdateFields) -> UpdateTaskRequest {
    UpdateTaskRequest {
        title: fields.title,
        description: None,
        clear_description: false,
        status_id: fields.status_id,
        priority: fields.priority,
        progress_pct: None,
        parent_task_id: None,
        clear_parent_task_id: false,
        milestone_id: None,
        clear_milestone_id: false,
        sprint_id: None,
        clear_sprint_id: false,
        soft_deadline: None,
        clear_soft_deadline: false,
        hard_deadline: None,
        clear_hard_deadline: false,
        estimated_minutes: None,
        clear_estimated_minutes: false,
        is_archived: None,
        label_ids: None,
        custom_field_values: None,
    }
}

/// 綴りは entity の `string_value` を正とする。CLI に一覧を写さない。
fn parse_priority(raw: &str) -> Result<TaskPriority> {
    TaskPriority::try_from_value(&raw.to_ascii_lowercase()).map_err(|_| {
        CliError::validation(format!(
            "unknown priority: {raw} (expected one of {})",
            TaskPriority::values().join(", ")
        ))
    })
}

fn tasks_path(api: &ApiClient, project_id: Uuid) -> Vec<String> {
    vec![
        "v1".into(),
        "tenants".into(),
        api.tenant_id().into(),
        "projects".into(),
        project_id.to_string(),
        "tasks".into(),
    ]
}

fn task_path(api: &ApiClient, project_id: Uuid, task_id: &str) -> Vec<String> {
    let mut segments = tasks_path(api, project_id);
    segments.push(task_id.to_string());
    segments
}

fn borrow(segments: &[String]) -> Vec<&str> {
    segments.iter().map(String::as_str).collect()
}

/// 参照から「どのプロジェクトのどのタスクか」を、API を呼ばずに決められる範囲で決める。
///
/// UUID 指定は所属プロジェクトを含まないので `--project` が要る。ここで弾かないと、
/// 直せる誤りが接続や設定の失敗に隠れる。
fn check_task_target(task_ref: &str, project_key: Option<&str>) -> Result<(String, String)> {
    match parse_task_ref(task_ref)? {
        TaskRef::Uuid(uuid) => {
            let project_key = project_key.ok_or_else(|| {
                CliError::validation("--project is required when using a task UUID")
            })?;
            Ok((project_key.to_string(), uuid.to_string()))
        }
        TaskRef::Seq {
            project_key,
            task_id,
        } => Ok((project_key, task_id)),
    }
}

async fn resolve_task_target(
    api: &ApiClient,
    (project_key, task_id): (String, String),
) -> Result<(ProjectResponse, String)> {
    Ok((resolve_project(api, &project_key).await?, task_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_priorities_in_the_form_the_api_documents() {
        assert_eq!(
            parse_priority("critical_fire").unwrap(),
            TaskPriority::CriticalFire
        );
        assert_eq!(parse_priority("Medium").unwrap(), TaskPriority::Medium);
    }

    #[test]
    fn rejects_an_unknown_priority_before_sending_it() {
        let err = parse_priority("urgent").unwrap_err();
        assert!(
            err.message.starts_with("unknown priority: urgent"),
            "{}",
            err.message
        );
        assert!(err.message.contains("critical_fire"), "{}", err.message);
        assert_eq!(err.exit_code, 2);
    }

    #[test]
    fn an_update_touches_only_the_fields_that_were_given() {
        let body = update_request(UpdateFields {
            title: Some("New".into()),
            status_id: None,
            priority: None,
        });
        let json = serde_json::to_value(&body).unwrap();

        assert_eq!(json["title"], "New");
        assert!(json["status_id"].is_null());
        // 解除は明示的な指定でしか起きない
        for key in [
            "clear_description",
            "clear_sprint_id",
            "clear_soft_deadline",
        ] {
            assert_eq!(json[key], false, "{key}");
        }
    }

    #[test]
    fn sends_a_priority_in_the_shape_the_request_body_expects() {
        // 一覧の絞り込み（クエリ）は snake_case、本文は enum の綴り。取り違えると 400 になる
        let body = update_request(UpdateFields {
            title: None,
            status_id: None,
            priority: Some(TaskPriority::CriticalFire),
        });
        assert_eq!(
            serde_json::to_value(&body).unwrap()["priority"],
            "CriticalFire"
        );
        assert_eq!(TaskPriority::CriticalFire.to_value(), "critical_fire");
    }
}
