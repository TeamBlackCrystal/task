//! 自分のタスク。

use payload::my_tasks::{MyTaskItem, MyTasksListResponse};
use payload::tasks::TaskDetailResponse;

use crate::Context;
use crate::api::ApiClient;
use crate::cli::MyCommand;
use crate::error::{CliError, Result};
use crate::output::{OutputOptions, print};
use crate::resolve::{TaskRef, find_done_status_id, parse_task_ref, resolve_project};

/// UUID から自分のタスクを探すときの 1 回あたりの取得数。
const PAGE: u64 = 200;

pub async fn run(context: &Context, command: MyCommand, output: OutputOptions) -> Result<i32> {
    match command {
        MyCommand::List { filter } => {
            let api = &context.connect()?;
            let tasks: MyTasksListResponse =
                api.get(&my_tasks_path(api), &[("filter", filter)]).await?;
            print(&tasks, output);
        }
        MyCommand::Complete { task_ref } => {
            // 参照の形は送る前に確かめる（設定不足より先に、直せる誤りを出す）
            let parsed = parse_task_ref(&task_ref)?;
            let api = &context.connect()?;
            let (project_id, task_id) = match parsed {
                TaskRef::Uuid(uuid) => {
                    let task = find_my_task(api, &uuid.to_string()).await?;
                    (task.project.id, task.id.to_string())
                }
                TaskRef::Seq {
                    project_key,
                    task_id,
                } => (resolve_project(api, &project_key).await?.id, task_id),
            };
            let status_id = find_done_status_id(api, project_id).await?;
            let project_id = project_id.to_string();
            let updated: TaskDetailResponse = api
                .put(
                    &[
                        "v1",
                        "tenants",
                        api.tenant_id(),
                        "projects",
                        &project_id,
                        "tasks",
                        &task_id,
                    ],
                    &crate::commands::tasks::done_request(status_id),
                )
                .await?;
            print(&updated, output);
        }
    }
    Ok(0)
}

fn my_tasks_path(api: &ApiClient) -> [&str; 6] {
    ["v1", "tenants", api.tenant_id(), "users", "me", "tasks"]
}

/// UUID だけでは所属プロジェクトが分からないので、自分のタスクを辿って突き合わせる。
async fn find_my_task(api: &ApiClient, id: &str) -> Result<MyTaskItem> {
    let mut offset = 0u64;
    loop {
        let page: MyTasksListResponse = api
            .get(
                &my_tasks_path(api),
                &[
                    ("filter", "all".into()),
                    ("limit", PAGE.to_string()),
                    ("offset", offset.to_string()),
                ],
            )
            .await?;
        let count = page.tasks.len() as u64;
        if let Some(task) = page
            .tasks
            .into_iter()
            .find(|task| task.id.to_string() == id)
        {
            return Ok(task);
        }
        if count < PAGE {
            return Err(CliError::not_found(format!("Task not found: {id}")));
        }
        offset += PAGE;
    }
}
