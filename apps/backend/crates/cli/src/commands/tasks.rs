//! タスク。

use chrono::{DateTime, NaiveDate, NaiveTime, TimeZone, Utc};
use entity::tasks::TaskPriority;
use payload::projects::ProjectResponse;
use payload::task_comments::{CreateCommentRequest, TaskCommentResponse};
use payload::task_extensions::SearchTasksResponse;
use payload::tasks::{
    AddAssigneeRequest, AssigneeInput, CreateTaskRequest, TaskAssigneeResponse,
    TaskAssigneeSummary, TaskDetailResponse, TaskListResponse, UpdateTaskRequest,
};
use sea_orm::ActiveEnum;
use serde_json::json;
use uuid::Uuid;

use crate::Context;
use crate::api::ApiClient;
use crate::cli::{TaskClearArgs, TaskFieldArgs, TasksCommand};
use crate::error::{CliError, Result};
use crate::output::{OutputOptions, print};
use crate::resolve::{
    TaskRef, default_status_id, find_done_status_id, parse_task_ref, resolve_label_id,
    resolve_milestone_id, resolve_project, resolve_sprint_id, resolve_status_id, resolve_user_id,
};

/// タスクの担当者に付く役割。API は自由文字列だが、画面と統合テストはこの綴りを使う。
const ASSIGNEE_ROLE: &str = "assignee";

/// サーバー側の上限。`list_tasks` は超過分を黙って 200 に丸めるので、CLI では手前で断る。
const MAX_LIST_LIMIT: u64 = 200;
/// `search_tasks` の上限。こちらも超過は黙って丸められる。
const MAX_SEARCH_LIMIT: u64 = 100;
use crate::text_input::{read_stdin, resolve_body};

pub async fn run(context: &Context, command: TasksCommand, output: OutputOptions) -> Result<i32> {
    match command {
        TasksCommand::List {
            project,
            priority,
            status,
            label,
            assignee,
            milestone,
            sprint,
            parent,
            archived,
            sort,
            limit,
            page,
        } => {
            // 一覧の絞り込みはクエリ文字列で受ける（`priority=medium`）。値の綴りは
            // 作成・更新の本文と同じなので、ここでも送信前に確かめる
            let priority = priority.as_deref().map(parse_priority).transpose()?;
            let sort = sort.as_deref().map(parse_sort).transpose()?;
            let limit = check_limit(limit, MAX_LIST_LIMIT)?;
            let offset = offset_for(page, limit)?;

            let api = &context.connect()?;
            let project = resolve_project(api, &project).await?;

            let mut query: Vec<(&str, String)> = vec![
                ("limit", limit.to_string()),
                ("offset", offset.to_string()),
                ("is_archived", archived.to_string()),
            ];
            if let Some(priority) = priority {
                query.push(("priority", priority.to_value()));
            }
            if let Some(sort) = sort {
                query.push(("sort", sort.to_string()));
            }
            if let Some(name) = &status {
                let id = resolve_status_id(api, project.id, name).await?;
                query.push(("status_id", id.to_string()));
            }
            if let Some(name) = &label {
                let id = resolve_label_id(api, project.id, name).await?;
                query.push(("label_id", id.to_string()));
            }
            if let Some(name) = &assignee {
                let id = resolve_user_id(api, project.id, name).await?;
                query.push(("assignee_id", id.to_string()));
            }
            if let Some(name) = &milestone {
                let id = resolve_milestone_id(api, project.id, name).await?;
                query.push(("milestone_id", id.to_string()));
            }
            if let Some(name) = &sprint {
                let id = resolve_sprint_id(api, project.id, name).await?;
                query.push(("sprint_id", id.to_string()));
            }
            if let Some(reference) = &parent {
                // 親は `KEY-N` でも指せるようにする。API は UUID を要求するので、
                // 詳細を 1 度引いて ID に直す
                let id = resolve_parent_task_id(api, &project, reference).await?;
                query.push(("parent_task_id", id.to_string()));
            }

            let tasks: TaskListResponse = api
                .get(&borrow(&tasks_path(api, project.id)), &query)
                .await?;
            if output.json {
                print(&tasks, output);
            } else {
                print_listing(&tasks, output, page, limit);
            }
        }
        TasksCommand::Search {
            query,
            project,
            limit,
            page,
        } => {
            let text = query.trim().to_string();
            if text.is_empty() {
                // 空で投げると API が 400 を返すだけなので、手前で理由を出す
                return Err(CliError::validation("Search query is required"));
            }
            let limit = check_limit(limit, MAX_SEARCH_LIMIT)?;
            let offset = offset_for(page, limit)?;

            let api = &context.connect()?;
            let project = resolve_project(api, &project).await?;
            let mut segments = tasks_path(api, project.id);
            segments.push("search".into());
            let hits: SearchTasksResponse = api
                .get(
                    &borrow(&segments),
                    &[
                        ("q", text),
                        ("limit", limit.to_string()),
                        ("offset", offset.to_string()),
                    ],
                )
                .await?;
            if output.json {
                print(&hits, output);
            } else {
                print_hits(&project, &hits, page, limit);
            }
        }
        TasksCommand::Create {
            project,
            title,
            description,
            description_file,
            priority,
            status,
            fields,
        } => {
            let priority = priority.as_deref().map(parse_priority).transpose()?;
            // 本文の読み取りは API を叩く前に済ませる（ファイルが無いときに
            // タスクを作ってから落ちるのを避ける）
            let description = resolve_body(description, description_file, "description")?;
            let api = &context.connect()?;
            let project = resolve_project(api, &project).await?;
            let status_id = match status {
                Some(status) => resolve_status_id(api, project.id, &status).await?,
                // 作成 API は status_id を必須で受ける。省略時に送らないと必ず 400 になる
                None => default_status_id(api, project.id).await?,
            };
            // 作成には「今あるものへの差分」が無いので、add/remove は受け付けない
            if !fields.add_labels.is_empty() || !fields.remove_labels.is_empty() {
                return Err(CliError::validation(
                    "--add-label / --remove-label work on an existing task; use --label when creating one",
                ));
            }
            let mut resolved = resolve_fields(api, &project, &fields).await?;
            let body = CreateTaskRequest {
                title,
                description,
                status_id,
                priority,
                progress_pct: resolved.progress_pct,
                parent_task_id: resolved.parent_task_id,
                milestone_id: resolved.milestone_id,
                sprint_id: resolved.sprint_id,
                soft_deadline: resolved.soft_deadline,
                hard_deadline: resolved.hard_deadline,
                estimated_minutes: resolved.estimated_minutes,
                assignees: resolved.assignees.take().unwrap_or_default(),
                label_ids: resolved.set_label_ids.take().unwrap_or_default(),
                custom_field_values: vec![],
            };
            check_deadline_order(body.soft_deadline, body.hard_deadline)?;
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
            description,
            description_file,
            status,
            priority,
            fields,
            clears,
            archive,
            unarchive,
        } => {
            let priority = priority.as_deref().map(parse_priority).transpose()?;
            let description = resolve_body(description, description_file, "description")?;
            let target = check_task_target(&task_ref, project.as_deref())?;
            let api = &context.connect()?;
            let (project, task_id) = resolve_task_target(api, target).await?;
            let status_id = match status {
                Some(status) => Some(resolve_status_id(api, project.id, &status).await?),
                None => None,
            };
            let resolved = resolve_fields(api, &project, &fields).await?;
            // ラベルの差分更新（--add-label / --remove-label）と担当者の置き換えは
            // 「今付いているもの」が要る。どちらも指定が無いなら詳細は引かない
            let merges_labels = !fields.add_labels.is_empty() || !fields.remove_labels.is_empty();
            let current: Option<TaskDetailResponse> =
                if merges_labels || resolved.assignees.is_some() {
                    Some(
                        api.get(&borrow(&task_path(api, project.id, &task_id)), &[])
                            .await?,
                    )
                } else {
                    None
                };
            let label_ids = match current.as_ref().filter(|_| merges_labels) {
                Some(current) => Some(merge_labels(
                    current.task.labels.iter().map(|label| label.id),
                    &resolved.add_label_ids,
                    &resolved.remove_label_ids,
                )),
                None => resolved.set_label_ids.clone(),
            };
            let body = update_request(UpdateFields {
                title,
                description,
                status_id,
                priority,
                resolved: &resolved,
                clears: &clears,
                label_ids,
                is_archived: archive.then_some(true).or(unarchive.then_some(false)),
            });
            check_deadline_order(body.soft_deadline, body.hard_deadline)?;
            // 担当者は更新の本文では動かせない。先にタスク本体を更新することで、
            // 本体の検証に失敗したときに担当者だけが変わる状態を避ける。
            let updated: TaskDetailResponse = api
                .put(&borrow(&task_path(api, project.id, &task_id)), &body)
                .await?;
            if let (Some(current), Some(desired)) = (&current, &resolved.assignees) {
                sync_assignees(api, project.id, &task_id, &current.task.assignees, desired).await?;
                // PUT の応答には専用 endpoint で反映した担当者がまだ含まれないため、
                // 成功時の出力は最終状態を再取得する。
                let final_task: TaskDetailResponse = api
                    .get(&borrow(&task_path(api, project.id, &task_id)), &[])
                    .await
                    .map_err(|error| {
                        CliError::new(format!(
                            "Task and assignees were updated, but fetching the final task failed: {}",
                            error.message
                        ))
                    })?;
                print(&final_task, output);
            } else {
                print(&updated, output);
            }
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
            body_file,
            project,
        } => {
            // 位置引数・ファイル・標準入力のいずれか。どれも無ければ空のまま送らない
            let body = match resolve_body(body, body_file, "comment body")? {
                Some(text) => text,
                None => read_stdin("comment body")?
                    .unwrap_or_default()
                    .trim_end_matches(['\n', '\r'])
                    .to_string(),
            };
            if body.is_empty() {
                // 引数・投入の検証エラーは 2（README の終了コード表）
                return Err(CliError::validation("Comment body is required"));
            }
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
        description: None,
        status_id: Some(status_id),
        priority: None,
        resolved: &ResolvedFields::empty(),
        clears: &TaskClearArgs::none(),
        label_ids: None,
        is_archived: None,
    })
}

struct UpdateFields<'a> {
    title: Option<String>,
    description: Option<String>,
    status_id: Option<Uuid>,
    priority: Option<TaskPriority>,
    resolved: &'a ResolvedFields,
    clears: &'a TaskClearArgs,
    label_ids: Option<Vec<Uuid>>,
    is_archived: Option<bool>,
}

/// 更新は「渡されたものだけ変える」。`clear_*` は明示的な解除用なので、
/// 指定されたときだけ立てる。未指定のフィールドは送らない（既存値を消さない）。
fn update_request(fields: UpdateFields) -> UpdateTaskRequest {
    let resolved = fields.resolved;
    let clears = fields.clears;
    UpdateTaskRequest {
        title: fields.title,
        description: fields.description,
        clear_description: clears.clear_description,
        status_id: fields.status_id,
        priority: fields.priority,
        progress_pct: resolved.progress_pct,
        parent_task_id: resolved.parent_task_id,
        clear_parent_task_id: clears.clear_parent,
        milestone_id: resolved.milestone_id,
        clear_milestone_id: clears.clear_milestone,
        sprint_id: resolved.sprint_id,
        clear_sprint_id: clears.clear_sprint,
        soft_deadline: resolved.soft_deadline,
        clear_soft_deadline: clears.clear_soft_deadline,
        hard_deadline: resolved.hard_deadline,
        clear_hard_deadline: clears.clear_hard_deadline,
        estimated_minutes: resolved.estimated_minutes,
        clear_estimated_minutes: clears.clear_estimate,
        is_archived: fields.is_archived,
        label_ids: fields.label_ids,
        custom_field_values: None,
    }
}

/// 名前・参照を ID に直したあとの値。作成と更新で同じものを使う。
#[derive(Default)]
struct ResolvedFields {
    soft_deadline: Option<DateTime<Utc>>,
    hard_deadline: Option<DateTime<Utc>>,
    estimated_minutes: Option<i32>,
    progress_pct: Option<i16>,
    parent_task_id: Option<Uuid>,
    milestone_id: Option<Uuid>,
    sprint_id: Option<Uuid>,
    /// `--label` による置き換え。未指定なら None（ラベルを触らない）
    set_label_ids: Option<Vec<Uuid>>,
    add_label_ids: Vec<Uuid>,
    remove_label_ids: Vec<Uuid>,
    assignees: Option<Vec<AssigneeInput>>,
}

impl ResolvedFields {
    fn empty() -> Self {
        Self::default()
    }
}

impl TaskClearArgs {
    /// 解除を一つも指定していない状態（`complete` のように本文を組み立てるだけの経路で使う）。
    fn none() -> Self {
        Self {
            clear_description: false,
            clear_soft_deadline: false,
            clear_hard_deadline: false,
            clear_estimate: false,
            clear_parent: false,
            clear_milestone: false,
            clear_sprint: false,
        }
    }
}

/// 名前で渡された値を ID に直す。API を呼ぶ前に範囲の検証も済ませる。
async fn resolve_fields(
    api: &ApiClient,
    project: &ProjectResponse,
    fields: &TaskFieldArgs,
) -> Result<ResolvedFields> {
    if let Some(progress) = fields.progress
        && !(0..=100).contains(&progress)
    {
        return Err(CliError::validation(format!(
            "--progress must be between 0 and 100 (got {progress})"
        )));
    }
    if let Some(estimate) = fields.estimate
        && estimate < 1
    {
        return Err(CliError::validation(format!(
            "--estimate must be 1 minute or more (got {estimate})"
        )));
    }

    let mut resolved = ResolvedFields {
        soft_deadline: fields
            .soft_deadline
            .as_deref()
            .map(|raw| parse_deadline("--soft-deadline", raw))
            .transpose()?,
        hard_deadline: fields
            .hard_deadline
            .as_deref()
            .map(|raw| parse_deadline("--hard-deadline", raw))
            .transpose()?,
        estimated_minutes: fields.estimate,
        progress_pct: fields.progress,
        ..ResolvedFields::default()
    };

    if let Some(reference) = &fields.parent {
        resolved.parent_task_id = Some(resolve_parent_task_id(api, project, reference).await?);
    }
    if let Some(name) = &fields.milestone {
        resolved.milestone_id = Some(resolve_milestone_id(api, project.id, name).await?);
    }
    if let Some(name) = &fields.sprint {
        resolved.sprint_id = Some(resolve_sprint_id(api, project.id, name).await?);
    }
    if !fields.labels.is_empty() {
        resolved.set_label_ids = Some(resolve_labels(api, project.id, &fields.labels).await?);
    }
    resolved.add_label_ids = resolve_labels(api, project.id, &fields.add_labels).await?;
    resolved.remove_label_ids = resolve_labels(api, project.id, &fields.remove_labels).await?;
    if !fields.assignees.is_empty() {
        let mut assignees = Vec::with_capacity(fields.assignees.len());
        for name in &fields.assignees {
            assignees.push(AssigneeInput {
                user_id: resolve_user_id(api, project.id, name).await?,
                role: ASSIGNEE_ROLE.to_string(),
            });
        }
        resolved.assignees = Some(assignees);
    }
    Ok(resolved)
}

/// 更新で `--assignee` に渡された顔ぶれへ寄せる。付け外しは専用の endpoint しかないので、
/// 今の担当者との差分を当てる。途中で失敗したら、成功済みの差分を逆向きに戻す。
async fn sync_assignees(
    api: &ApiClient,
    project_id: Uuid,
    task_id: &str,
    current: &[TaskAssigneeSummary],
    desired: &[AssigneeInput],
) -> Result<()> {
    let (added, removed) = assignee_changes(
        current.iter().map(|assignee| assignee.user.id),
        desired.iter().map(|assignee| assignee.user_id),
    );
    let mut applied_adds = Vec::new();
    let mut applied_removes = Vec::new();

    for user_id in added {
        // 役割は作成のときと同じ綴りを使う（`--assignee` は役割を受けない）
        let role = desired
            .iter()
            .find(|assignee| assignee.user_id == user_id)
            .map_or(ASSIGNEE_ROLE, |assignee| assignee.role.as_str());
        if let Err(error) = add_assignee(api, project_id, task_id, user_id, role).await {
            return rollback_assignee_sync(
                api,
                project_id,
                task_id,
                current,
                &applied_adds,
                &applied_removes,
                error,
            )
            .await;
        }
        applied_adds.push(user_id);
    }
    for user_id in removed {
        if let Err(error) = remove_assignee(api, project_id, task_id, user_id).await {
            return rollback_assignee_sync(
                api,
                project_id,
                task_id,
                current,
                &applied_adds,
                &applied_removes,
                error,
            )
            .await;
        }
        applied_removes.push(user_id);
    }
    Ok(())
}

async fn add_assignee(
    api: &ApiClient,
    project_id: Uuid,
    task_id: &str,
    user_id: Uuid,
    role: &str,
) -> Result<()> {
    let mut segments = task_path(api, project_id, task_id);
    segments.push("assignees".into());
    let _: TaskAssigneeResponse = api
        .post(
            &borrow(&segments),
            &AddAssigneeRequest {
                user_id,
                role: role.to_string(),
            },
        )
        .await?;
    Ok(())
}

async fn remove_assignee(
    api: &ApiClient,
    project_id: Uuid,
    task_id: &str,
    user_id: Uuid,
) -> Result<()> {
    let mut segments = task_path(api, project_id, task_id);
    segments.push("assignees".into());
    segments.push(user_id.to_string());
    api.delete(&borrow(&segments)).await
}

/// 担当者同期が途中で失敗したとき、本体更新後に残った部分更新を可能な範囲で戻す。
async fn rollback_assignee_sync(
    api: &ApiClient,
    project_id: Uuid,
    task_id: &str,
    current: &[TaskAssigneeSummary],
    applied_adds: &[Uuid],
    applied_removes: &[Uuid],
    original_error: CliError,
) -> Result<()> {
    let mut rollback_errors = Vec::new();

    for user_id in applied_adds.iter().rev().copied() {
        if let Err(error) = remove_assignee(api, project_id, task_id, user_id).await {
            rollback_errors.push(format!("remove {user_id}: {}", error.message));
        }
    }

    for user_id in applied_removes.iter().rev().copied() {
        let Some(previous) = current.iter().find(|assignee| assignee.user.id == user_id) else {
            rollback_errors.push(format!(
                "restore {user_id}: original assignee was not found"
            ));
            continue;
        };
        if let Err(error) = add_assignee(api, project_id, task_id, user_id, &previous.role).await {
            rollback_errors.push(format!("restore {user_id}: {}", error.message));
        }
    }

    if rollback_errors.is_empty() {
        let mut error = original_error;
        error.message = format!(
            "Task fields were updated, but assignee synchronization failed; assignees were restored: {}",
            error.message
        );
        Err(error)
    } else {
        Err(CliError::new(format!(
            "Task fields were updated, but assignee synchronization failed: {}; rollback was incomplete: {}",
            original_error.message,
            rollback_errors.join("; ")
        )))
    }
}

/// 今の担当者を頼まれた顔ぶれに合わせるための「足す・外す」。
/// 既にいる利用者は触らない（外して付け直すと通知と履歴が余計に出る）。
fn assignee_changes(
    current: impl IntoIterator<Item = Uuid>,
    desired: impl IntoIterator<Item = Uuid>,
) -> (Vec<Uuid>, Vec<Uuid>) {
    let current: Vec<Uuid> = current.into_iter().collect();
    let desired: Vec<Uuid> = desired.into_iter().collect();
    let mut added: Vec<Uuid> = Vec::new();
    for user_id in &desired {
        if !current.contains(user_id) && !added.contains(user_id) {
            added.push(*user_id);
        }
    }
    let removed = current
        .into_iter()
        .filter(|user_id| !desired.contains(user_id))
        .collect();
    (added, removed)
}

async fn resolve_labels(api: &ApiClient, project_id: Uuid, names: &[String]) -> Result<Vec<Uuid>> {
    let mut ids = Vec::with_capacity(names.len());
    for name in names {
        ids.push(resolve_label_id(api, project_id, name).await?);
    }
    Ok(ids)
}

/// 差分指定を今のラベルへ当てる。付け外しの順に依らないよう、外すほうを後に見る。
fn merge_labels(
    current: impl IntoIterator<Item = Uuid>,
    add: &[Uuid],
    remove: &[Uuid],
) -> Vec<Uuid> {
    let mut merged: Vec<Uuid> = current.into_iter().collect();
    for id in add {
        if !merged.contains(id) {
            merged.push(*id);
        }
    }
    merged.retain(|id| !remove.contains(id));
    merged
}

/// 親タスクは `KEY-N` でも指せる。API は UUID を要求するので詳細を 1 度引く。
async fn resolve_parent_task_id(
    api: &ApiClient,
    project: &ProjectResponse,
    reference: &str,
) -> Result<Uuid> {
    match parse_task_ref(reference)? {
        TaskRef::Uuid(uuid) => Ok(uuid),
        TaskRef::Seq { task_id, .. } => {
            let parent: TaskDetailResponse = api
                .get(&borrow(&task_path(api, project.id, &task_id)), &[])
                .await?;
            Ok(parent.task.id)
        }
    }
}

/// 期限は RFC 3339、または `YYYY-MM-DD`（その日の終わりを UTC で取る）。
fn parse_deadline(flag: &str, raw: &str) -> Result<DateTime<Utc>> {
    if let Ok(parsed) = DateTime::parse_from_rfc3339(raw) {
        return Ok(parsed.with_timezone(&Utc));
    }
    if let Ok(date) = NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
        let end_of_day = NaiveTime::from_hms_opt(23, 59, 59).expect("valid end-of-day time");
        return Ok(Utc.from_utc_datetime(&date.and_time(end_of_day)));
    }
    Err(CliError::validation(format!(
        "{flag}: expected RFC 3339 (2026-09-30T12:00:00Z) or a date (2026-09-30), got {raw}"
    )))
}

/// API も同じ関係を弾くが、往復する前に理由を出す。
fn check_deadline_order(soft: Option<DateTime<Utc>>, hard: Option<DateTime<Utc>>) -> Result<()> {
    if let (Some(soft), Some(hard)) = (soft, hard)
        && soft >= hard
    {
        return Err(CliError::validation(
            "--soft-deadline must be earlier than --hard-deadline",
        ));
    }
    Ok(())
}

/// 並び順。綴りを外したときに何が使えるか出す。
fn parse_sort(raw: &str) -> Result<&'static str> {
    const SORTS: [&str; 3] = ["created_at_desc", "priority_asc", "deadline_asc"];
    SORTS
        .iter()
        .find(|sort| sort.eq_ignore_ascii_case(raw))
        .copied()
        .ok_or_else(|| {
            CliError::validation(format!(
                "unknown sort: {raw} (expected one of {})",
                SORTS.join(", ")
            ))
        })
}

/// サーバーは上限を超えた要求を黙って丸めるので、頼んだ件数と返る件数がずれる。
/// 気付けないままページを送ると行が飛ぶので、手前で断る。
fn check_limit(limit: u64, max: u64) -> Result<u64> {
    if limit == 0 || limit > max {
        return Err(CliError::validation(format!(
            "--limit must be between 1 and {max} (got {limit})"
        )));
    }
    Ok(limit)
}

fn offset_for(page: u64, limit: u64) -> Result<u64> {
    if page == 0 {
        return Err(CliError::validation("--page starts at 1"));
    }
    Ok((page - 1) * limit)
}

/// 人間向けの一覧。総件数を出さないと、既定の 50 件で切れていることに気付けない。
fn print_listing(tasks: &TaskListResponse, output: OutputOptions, page: u64, limit: u64) {
    if !tasks.tasks.is_empty() {
        print(tasks, output);
    }
    println!(
        "{}",
        page_summary(tasks.tasks.len(), tasks.total, page, limit)
    );
}

fn print_hits(project: &ProjectResponse, hits: &SearchTasksResponse, page: u64, limit: u64) {
    for hit in &hits.tasks {
        println!("{}-{}	{}", project.key, hit.seq_id, hit.title);
        let snippet = plain_snippet(&hit.highlight);
        if !snippet.trim().is_empty() {
            println!("	{}", snippet.trim());
        }
    }
    println!(
        "{}",
        page_summary(hits.tasks.len(), hits.total, page, limit)
    );
}

/// 検索の抜粋は画面用の HTML（一致箇所が `<em>`、本文は実体参照）で返る。
/// 端末にはタグを外し、実体参照を元の文字へ戻して出す。`--json` は API の値のまま。
fn plain_snippet(highlight: &str) -> String {
    const ENTITIES: [(&str, char); 5] = [
        ("&amp;", '&'),
        ("&lt;", '<'),
        ("&gt;", '>'),
        ("&quot;", '"'),
        ("&#39;", '\''),
    ];
    // 元テキストの `<em>` は `&lt;em&gt;` になっているので、タグを先に外して取り違えない
    let without_tags = highlight.replace("<em>", "").replace("</em>", "");
    let mut out = String::with_capacity(without_tags.len());
    let mut rest = without_tags.as_str();
    // `&amp;lt;` のような二重の escape を戻しすぎないよう、左から一度だけ読む
    while let Some(index) = rest.find('&') {
        out.push_str(&rest[..index]);
        rest = &rest[index..];
        match ENTITIES.iter().find(|(entity, _)| rest.starts_with(entity)) {
            Some((entity, character)) => {
                out.push(*character);
                rest = &rest[entity.len()..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn page_summary(shown: usize, total: u64, page: u64, limit: u64) -> String {
    let seen = page.saturating_sub(1).saturating_mul(limit) + shown as u64;
    if seen < total {
        format!(
            "{shown} 件表示 / 全 {total} 件（--page {} で続き）",
            page + 1
        )
    } else {
        format!("{shown} 件表示 / 全 {total} 件")
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

    /// 追加した項目を「何も指定していない」状態にした更新本文。
    /// 既存の 4 項目だけを見るテストは、これを通して組み立てる。
    fn update_body(
        title: Option<&str>,
        description: Option<&str>,
        priority: Option<TaskPriority>,
    ) -> UpdateTaskRequest {
        let resolved = ResolvedFields::empty();
        let clears = TaskClearArgs::none();
        update_request(UpdateFields {
            title: title.map(Into::into),
            description: description.map(Into::into),
            status_id: None,
            priority,
            resolved: &resolved,
            clears: &clears,
            label_ids: None,
            is_archived: None,
        })
    }

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
    fn an_update_sends_the_description_when_it_is_given() {
        let body = update_body(None, Some("## 見出し\n\n本文"), None);
        let json = serde_json::to_value(&body).unwrap();

        assert_eq!(json["description"], "## 見出し\n\n本文");
        // 本文を渡しただけで解除が立ってはいけない
        assert_eq!(json["clear_description"], false);
        assert!(json["title"].is_null());
    }

    /// 空文字は「本文を空にする」意図。`clear_description` とは別扱いで、そのまま送る。
    #[test]
    fn an_update_sends_an_empty_description_as_is() {
        let body = update_body(None, Some(""), None);
        let json = serde_json::to_value(&body).unwrap();

        assert_eq!(json["description"], "");
        assert_eq!(json["clear_description"], false);
    }

    #[test]
    fn an_update_touches_only_the_fields_that_were_given() {
        let body = update_body(Some("New"), None, None);
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
        let body = update_body(None, None, Some(TaskPriority::CriticalFire));
        assert_eq!(
            serde_json::to_value(&body).unwrap()["priority"],
            "CriticalFire"
        );
        assert_eq!(TaskPriority::CriticalFire.to_value(), "critical_fire");
    }

    #[test]
    fn rejects_a_page_size_the_server_would_silently_round_down() {
        // 201 を頼むとサーバーは黙って 200 にする。気付けないままページを送ると行が飛ぶ
        assert_eq!(check_limit(1, MAX_LIST_LIMIT).unwrap(), 1);
        assert_eq!(check_limit(200, MAX_LIST_LIMIT).unwrap(), 200);
        for bad in [0, 201] {
            let err = check_limit(bad, MAX_LIST_LIMIT).unwrap_err();
            assert!(err.message.contains("between 1 and 200"), "{}", err.message);
            assert_eq!(err.exit_code, 2, "{bad}");
        }
        // 検索は別の上限を持つ
        assert_eq!(check_limit(100, MAX_SEARCH_LIMIT).unwrap(), 100);
        assert!(check_limit(101, MAX_SEARCH_LIMIT).is_err());
    }

    #[test]
    fn counts_pages_from_one() {
        assert_eq!(offset_for(1, 50).unwrap(), 0);
        assert_eq!(offset_for(2, 50).unwrap(), 50);
        assert_eq!(offset_for(3, 20).unwrap(), 40);
        assert_eq!(offset_for(0, 50).unwrap_err().exit_code, 2);
    }

    #[test]
    fn points_at_the_next_page_only_while_rows_remain() {
        // 既定の 50 件で切れていることに気付けないと、古いタスクを見落とす
        assert_eq!(
            page_summary(50, 181, 1, 50),
            "50 件表示 / 全 181 件（--page 2 で続き）"
        );
        assert_eq!(page_summary(31, 181, 4, 50), "31 件表示 / 全 181 件");
        assert_eq!(page_summary(0, 181, 9, 50), "0 件表示 / 全 181 件");
        assert_eq!(page_summary(0, 0, 1, 50), "0 件表示 / 全 0 件");
    }

    #[test]
    fn reads_the_sort_names_the_api_accepts_and_lists_them_when_wrong() {
        for sort in ["created_at_desc", "priority_asc", "deadline_asc"] {
            assert_eq!(parse_sort(sort).unwrap(), sort);
        }
        assert_eq!(parse_sort("DEADLINE_ASC").unwrap(), "deadline_asc");

        let err = parse_sort("due").unwrap_err();
        assert!(
            err.message.starts_with("unknown sort: due"),
            "{}",
            err.message
        );
        assert!(err.message.contains("created_at_desc"), "{}", err.message);
        assert_eq!(err.exit_code, 2);
    }

    #[test]
    fn reads_a_bare_date_at_the_end_of_that_day() {
        let parsed = parse_deadline("--soft-deadline", "2026-09-30").unwrap();
        assert_eq!(parsed.to_rfc3339(), "2026-09-30T23:59:59+00:00");

        let exact = parse_deadline("--hard-deadline", "2026-09-30T12:00:00Z").unwrap();
        assert_eq!(exact.to_rfc3339(), "2026-09-30T12:00:00+00:00");

        // 別の時間帯で渡されても UTC に揃える
        let offset = parse_deadline("--hard-deadline", "2026-09-30T12:00:00+09:00").unwrap();
        assert_eq!(offset.to_rfc3339(), "2026-09-30T03:00:00+00:00");
    }

    #[test]
    fn rejects_a_deadline_that_is_not_a_date() {
        for bad in ["2026-13-01", "30/09/2026", "tomorrow", ""] {
            let err = parse_deadline("--soft-deadline", bad).unwrap_err();
            assert!(
                err.message.contains("--soft-deadline"),
                "{bad}: {}",
                err.message
            );
            assert_eq!(err.exit_code, 2, "{bad}");
        }
    }

    #[test]
    fn refuses_a_soft_deadline_that_is_not_earlier_than_the_hard_one() {
        let soft = parse_deadline("--soft-deadline", "2026-10-02").unwrap();
        let hard = parse_deadline("--hard-deadline", "2026-10-01").unwrap();

        assert!(check_deadline_order(Some(soft), Some(hard)).is_err());
        assert!(check_deadline_order(Some(hard), Some(soft)).is_ok());
        // API は同時刻も 400 にする。手前で同じ関係を弾く
        let err = check_deadline_order(Some(soft), Some(soft)).unwrap_err();
        assert!(err.message.contains("earlier than"), "{}", err.message);
        assert_eq!(err.exit_code, 2);
        assert!(check_deadline_order(Some(soft), None).is_ok());
        assert!(check_deadline_order(None, None).is_ok());
    }

    #[test]
    fn applies_label_changes_without_dropping_the_others() {
        let (kept, added, removed) = (uuid(1), uuid(2), uuid(3));

        assert_eq!(
            merge_labels([kept, removed], &[added], &[removed]),
            vec![kept, added]
        );
        // 既に付いているものを足しても重複させない
        assert_eq!(merge_labels([kept], &[kept], &[]), vec![kept]);
        // 付いていないものを外しても落ちない
        assert_eq!(merge_labels([kept], &[], &[removed]), vec![kept]);
        // 同じ ID を足して外したら、外すほうが勝つ
        assert_eq!(merge_labels([], &[added], &[added]), Vec::<Uuid>::new());
    }

    /// 未指定のラベルで空配列を送ると、API 側は「全解除」と解釈する。
    #[test]
    fn leaves_labels_alone_when_none_were_given() {
        let body = update_body(Some("New"), None, None);
        let json = serde_json::to_value(&body).unwrap();

        assert!(json["label_ids"].is_null(), "{}", json["label_ids"]);
        assert!(json["custom_field_values"].is_null());
        assert!(json["is_archived"].is_null());
    }

    #[test]
    fn sends_the_resolved_fields_under_the_keys_the_api_expects() {
        let resolved = ResolvedFields {
            soft_deadline: Some(parse_deadline("--soft-deadline", "2026-09-30").unwrap()),
            hard_deadline: Some(parse_deadline("--hard-deadline", "2026-10-31").unwrap()),
            estimated_minutes: Some(90),
            progress_pct: Some(40),
            parent_task_id: Some(uuid(7)),
            milestone_id: Some(uuid(8)),
            sprint_id: Some(uuid(9)),
            ..ResolvedFields::default()
        };
        let clears = TaskClearArgs::none();
        let body = update_request(UpdateFields {
            title: None,
            description: None,
            status_id: None,
            priority: None,
            resolved: &resolved,
            clears: &clears,
            label_ids: Some(vec![uuid(1)]),
            is_archived: Some(true),
        });
        let json = serde_json::to_value(&body).unwrap();

        assert_eq!(json["soft_deadline"], "2026-09-30T23:59:59Z");
        assert_eq!(json["hard_deadline"], "2026-10-31T23:59:59Z");
        assert_eq!(json["estimated_minutes"], 90);
        assert_eq!(json["progress_pct"], 40);
        assert_eq!(json["parent_task_id"], uuid(7).to_string());
        assert_eq!(json["milestone_id"], uuid(8).to_string());
        assert_eq!(json["sprint_id"], uuid(9).to_string());
        assert_eq!(json["label_ids"][0], uuid(1).to_string());
        assert_eq!(json["is_archived"], true);
    }

    #[test]
    fn raises_the_clear_flags_only_for_the_ones_that_were_asked_for() {
        let resolved = ResolvedFields::empty();
        let clears = TaskClearArgs {
            clear_sprint: true,
            ..TaskClearArgs::none()
        };
        let body = update_request(UpdateFields {
            title: None,
            description: None,
            status_id: None,
            priority: None,
            resolved: &resolved,
            clears: &clears,
            label_ids: None,
            is_archived: None,
        });
        let json = serde_json::to_value(&body).unwrap();

        assert_eq!(json["clear_sprint_id"], true);
        for key in [
            "clear_description",
            "clear_parent_task_id",
            "clear_milestone_id",
            "clear_soft_deadline",
            "clear_hard_deadline",
            "clear_estimated_minutes",
        ] {
            assert_eq!(json[key], false, "{key}");
        }
    }

    /// `--assignee` は顔ぶれの置き換え。更新の本文には担当者が無いので、差分を別に当てる。
    #[test]
    fn turns_the_requested_assignees_into_what_to_add_and_remove() {
        let (kept, added, removed) = (uuid(1), uuid(2), uuid(3));

        let (add, remove) = assignee_changes([kept, removed], [kept, added]);
        assert_eq!(add, vec![added]);
        assert_eq!(remove, vec![removed]);

        // 既にいる利用者は触らない（外して付け直すと通知と履歴が余計に出る）
        let (add, remove) = assignee_changes([kept], [kept]);
        assert!(add.is_empty(), "{add:?}");
        assert!(remove.is_empty(), "{remove:?}");

        // 同じ利用者を 2 回渡しても足すのは 1 度
        let (add, _) = assignee_changes([], [added, added]);
        assert_eq!(add, vec![added]);

        // 誰も指定していない状態は「全員外す」
        let (add, remove) = assignee_changes([kept, removed], []);
        assert!(add.is_empty(), "{add:?}");
        assert_eq!(remove, vec![kept, removed]);
    }

    /// 抜粋は画面用の HTML。端末にタグや実体参照が出ると読めない。
    #[test]
    fn shows_a_search_snippet_as_plain_text() {
        assert_eq!(
            plain_snippet("…直す<em>ページ</em>ング&amp;検索…"),
            "…直すページング&検索…"
        );
        assert_eq!(
            plain_snippet("&lt;em&gt;タグ&lt;/em&gt; と &quot;引用&quot; と &#39;単引用&#39;"),
            "<em>タグ</em> と \"引用\" と '単引用'"
        );
        // 元テキストの `&lt;` は二重に escape されて届く。戻すのは 1 段だけ
        assert_eq!(plain_snippet("&amp;lt;"), "&lt;");
        // 実体参照でない `&` はそのまま残す
        assert_eq!(plain_snippet("A & B &notanentity;"), "A & B &notanentity;");
        assert_eq!(plain_snippet(""), "");
    }

    fn uuid(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }
}
