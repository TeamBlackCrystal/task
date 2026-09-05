use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use axum_valid::Valid;
use chrono::Utc;
use sea_orm::sea_query::{Expr, LockType};
use sea_orm::{
    ActiveModelTrait,
    ActiveValue::Set,
    ColumnTrait, Condition, ConnectionTrait, EntityTrait, IsolationLevel, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, TransactionTrait,
    prelude::{DateTimeWithTimeZone, Uuid},
};
use std::collections::{HashMap, HashSet, VecDeque};

use crate::AppState;
use crate::auth_helpers::{is_tenant_owner, require_project_access};
use crate::error::AppError;
use crate::extractors::AuthUser;
use crate::openapi::CrudErrors;
use common::cursor::{decode_cursor, encode_cursor};
use entity::{
    labels, milestones, project_statuses, sprints, task_assignees, task_labels, task_relations,
    tasks,
};
use payload::tasks::*;
use service::custom_fields::{
    ensure_required_custom_fields, load_task_custom_field_values, upsert_task_custom_field_values,
};
use service::db::is_postgres_unique_violation;
use service::notifications::{notify_assigned, notify_status_changed};
use service::task_activities::{
    priority_label, record_activity, record_label_diff, status_name, task_label_entries,
};
use service::task_responses::{build_task_response, build_task_responses};

// ─── Task lookup (UUID or KEY-N) ─────────────────────────────────────────

pub(crate) async fn resolve_task(
    state: &AppState,
    tenant_id: Uuid,
    project_id: Uuid,
    id_str: &str,
) -> Result<tasks::Model, AppError> {
    if let Ok(uuid) = id_str.parse::<Uuid>() {
        return tasks::Entity::find_by_id(uuid)
            .filter(tasks::Column::ProjectId.eq(project_id))
            .filter(tasks::Column::DeletedAt.is_null())
            .one(&state.db)
            .await?
            .ok_or(AppError::NotFound);
    }
    let project = entity::projects::Entity::find_by_id(project_id)
        .filter(entity::projects::Column::TenantId.eq(tenant_id))
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    let prefix = format!("{}-", project.key);
    if let Some(n_str) = id_str.strip_prefix(&prefix)
        && let Ok(seq) = n_str.parse::<i32>()
    {
        return tasks::Entity::find()
            .filter(tasks::Column::ProjectId.eq(project_id))
            .filter(tasks::Column::SeqId.eq(seq))
            .filter(tasks::Column::DeletedAt.is_null())
            .one(&state.db)
            .await?
            .ok_or(AppError::NotFound);
    }
    Err(AppError::NotFound)
}

async fn record_task_field_activities<C: ConnectionTrait>(
    db: &C,
    task_id: Uuid,
    user_id: Uuid,
    project_id: Uuid,
    before: &tasks::Model,
    payload: &UpdateTaskRequest,
) -> Result<(), AppError> {
    if let Some(new_status_id) = payload.status_id
        && new_status_id != before.status_id
    {
        let from = status_name(db, before.status_id).await?;
        let to = status_name(db, new_status_id).await?;
        record_activity(
            db,
            task_id,
            Some(user_id),
            "status_changed",
            serde_json::json!({ "from": from, "to": to }),
        )
        .await?;
        notify_status_changed(db, project_id, task_id, user_id, &from, &to).await?;
    }
    if let Some(new_priority) = payload.priority
        && new_priority != before.priority
    {
        record_activity(
            db,
            task_id,
            Some(user_id),
            "priority_changed",
            serde_json::json!({
                "from": priority_label(before.priority),
                "to": priority_label(new_priority),
            }),
        )
        .await?;
    }
    if payload.clear_soft_deadline || payload.soft_deadline.is_some() {
        let before_soft = before.soft_deadline.map(|dt| dt.with_timezone(&Utc));
        let new_soft = if payload.clear_soft_deadline {
            None
        } else {
            payload.soft_deadline.or(before_soft)
        };
        if new_soft != before_soft {
            record_activity(
                db,
                task_id,
                Some(user_id),
                "deadline_changed",
                serde_json::json!({
                    "field": "soft_deadline",
                    "from": before_soft,
                    "to": new_soft,
                }),
            )
            .await?;
        }
    }
    if payload.clear_hard_deadline || payload.hard_deadline.is_some() {
        let before_hard = before.hard_deadline.map(|dt| dt.with_timezone(&Utc));
        let new_hard = if payload.clear_hard_deadline {
            None
        } else {
            payload.hard_deadline.or(before_hard)
        };
        if new_hard != before_hard {
            record_activity(
                db,
                task_id,
                Some(user_id),
                "deadline_changed",
                serde_json::json!({
                    "field": "hard_deadline",
                    "from": before_hard,
                    "to": new_hard,
                }),
            )
            .await?;
        }
    }
    if let Some(archived) = payload.is_archived
        && archived != before.is_archived
    {
        let event_type = if archived { "archived" } else { "unarchived" };
        record_activity(
            db,
            task_id,
            Some(user_id),
            event_type,
            serde_json::json!({}),
        )
        .await?;
    }
    let _ = project_id;
    Ok(())
}

// ─── BFS cycle detection ─────────────────────────────────────────────────

async fn would_create_cycle<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    blocker: Uuid,
    blocked: Uuid,
) -> Result<bool, AppError> {
    let project_task_ids: HashSet<Uuid> = tasks::Entity::find()
        .filter(tasks::Column::ProjectId.eq(project_id))
        .filter(tasks::Column::DeletedAt.is_null())
        .all(db)
        .await?
        .into_iter()
        .map(|t| t.id)
        .collect();
    let task_id_vec: Vec<Uuid> = project_task_ids.iter().copied().collect();
    if task_id_vec.is_empty() {
        return Ok(false);
    }
    let all_rels = task_relations::Entity::find()
        .filter(
            Condition::any()
                .add(task_relations::Column::BlockerTaskId.is_in(task_id_vec.clone()))
                .add(task_relations::Column::BlockedTaskId.is_in(task_id_vec)),
        )
        .all(db)
        .await?;
    let mut graph: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for rel in &all_rels {
        if project_task_ids.contains(&rel.blocker_task_id)
            && project_task_ids.contains(&rel.blocked_task_id)
        {
            graph
                .entry(rel.blocker_task_id)
                .or_default()
                .push(rel.blocked_task_id);
        }
    }
    let mut visited: HashSet<Uuid> = HashSet::new();
    let mut queue: VecDeque<Uuid> = VecDeque::new();
    queue.push_back(blocked);
    while let Some(cur) = queue.pop_front() {
        if cur == blocker {
            return Ok(true);
        }
        if visited.insert(cur)
            && let Some(nexts) = graph.get(&cur)
        {
            queue.extend(nexts);
        }
    }
    Ok(false)
}

// ─── Parent hierarchy cycle detection ────────────────────────────────────

/// `ancestor_id` が `descendant_id` の祖先か（parent_task_id リンクを上方向に
/// 辿って到達するか）を判定する。`ancestor_id == descendant_id` の場合も true。
/// parent_task_id に循環を作る更新を防ぐために使用する。
async fn is_ancestor_of<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    ancestor_id: Uuid,
    descendant_id: Uuid,
) -> Result<bool, AppError> {
    if ancestor_id == descendant_id {
        return Ok(true);
    }
    let mut visited: HashSet<Uuid> = HashSet::new();
    let mut current = descendant_id;
    loop {
        if !visited.insert(current) {
            return Ok(false);
        }
        let node = tasks::Entity::find_by_id(current)
            .filter(tasks::Column::ProjectId.eq(project_id))
            .one(db)
            .await?;
        match node.and_then(|t| t.parent_task_id) {
            None => return Ok(false),
            Some(parent_id) => {
                if parent_id == ancestor_id {
                    return Ok(true);
                }
                current = parent_id;
            }
        }
    }
}

fn parse_task_priority(value: &str) -> Result<tasks::TaskPriority, AppError> {
    match value {
        "critical_fire" => Ok(tasks::TaskPriority::CriticalFire),
        "critical" => Ok(tasks::TaskPriority::Critical),
        "high" => Ok(tasks::TaskPriority::High),
        "medium" => Ok(tasks::TaskPriority::Medium),
        "low" => Ok(tasks::TaskPriority::Low),
        "trivial" => Ok(tasks::TaskPriority::Trivial),
        _ => Err(AppError::BadRequest),
    }
}

/// 一覧のカーソル。並び順の第一キーを持ち、`sort` を跨いだ使い回しを弾くために
/// どの並びで作られたかも一緒に運ぶ。`id` は全変種で `DESC` のタイブレーカー。
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "sort")]
enum TaskCursor {
    #[serde(rename = "created_at_desc")]
    CreatedAtDesc {
        created_at: chrono::DateTime<chrono::Utc>,
        id: Uuid,
    },
    #[serde(rename = "priority_asc")]
    PriorityAsc {
        priority: tasks::TaskPriority,
        id: Uuid,
    },
    #[serde(rename = "deadline_asc")]
    DeadlineAsc {
        /// 期限なしは `null`。Postgres の `ASC` は NULL が最後なので、
        /// `null` のカーソルは「末尾の期限なしの塊の途中」を指す
        soft_deadline: Option<chrono::DateTime<chrono::Utc>>,
        id: Uuid,
    },
}

/// 一覧の並び。`sort` の文字列を 1 度だけ解釈し、順序とカーソルの形を 1 か所に揃える。
#[derive(Clone, Copy, PartialEq, Eq)]
enum TaskSort {
    CreatedAtDesc,
    PriorityAsc,
    DeadlineAsc,
}

impl TaskSort {
    /// 知らない値は既定（作成日の新しい順）に倒す。既存の挙動を変えない
    fn parse(raw: Option<&str>) -> Self {
        match raw.unwrap_or("created_at_desc") {
            "priority_asc" => Self::PriorityAsc,
            "deadline_asc" => Self::DeadlineAsc,
            _ => Self::CreatedAtDesc,
        }
    }

    fn cursor_of(self, task: &tasks::Model) -> String {
        encode_cursor(&match self {
            Self::CreatedAtDesc => TaskCursor::CreatedAtDesc {
                created_at: task.created_at.with_timezone(&chrono::Utc),
                id: task.id,
            },
            Self::PriorityAsc => TaskCursor::PriorityAsc {
                priority: task.priority,
                id: task.id,
            },
            Self::DeadlineAsc => TaskCursor::DeadlineAsc {
                soft_deadline: task.soft_deadline.map(|d| d.with_timezone(&chrono::Utc)),
                id: task.id,
            },
        })
    }

    /// 「このカーソルより後ろ」の条件。`ORDER BY` と同じ形にしないと行が飛ぶ。
    ///
    /// 並びを変えるとキーの意味が変わるので、別の並びで作られたカーソルは 400 で断る
    /// （黙って先頭へ戻すと、並び替えのたびに一覧が巻き戻る）。
    fn keyset_after(self, cursor: TaskCursor) -> Result<Condition, AppError> {
        Ok(match (self, cursor) {
            (Self::CreatedAtDesc, TaskCursor::CreatedAtDesc { created_at, id }) => {
                let at: DateTimeWithTimeZone = created_at.into();
                Condition::any().add(tasks::Column::CreatedAt.lt(at)).add(
                    Condition::all()
                        .add(tasks::Column::CreatedAt.eq(at))
                        .add(tasks::Column::Id.lt(id)),
                )
            }
            (Self::PriorityAsc, TaskCursor::PriorityAsc { priority, id }) => Condition::any()
                .add(tasks::Column::Priority.gt(priority))
                .add(
                    Condition::all()
                        .add(tasks::Column::Priority.eq(priority))
                        .add(tasks::Column::Id.lt(id)),
                ),
            // 期限ありの途中。期限なしは並びの末尾なので、まとめて「後ろ」に含める
            (
                Self::DeadlineAsc,
                TaskCursor::DeadlineAsc {
                    soft_deadline: Some(deadline),
                    id,
                },
            ) => {
                let at: DateTimeWithTimeZone = deadline.into();
                Condition::any()
                    .add(tasks::Column::SoftDeadline.gt(at))
                    .add(tasks::Column::SoftDeadline.is_null())
                    .add(
                        Condition::all()
                            .add(tasks::Column::SoftDeadline.eq(at))
                            .add(tasks::Column::Id.lt(id)),
                    )
            }
            // 期限なしの塊の途中。ここから先は期限なししか残っていない
            (
                Self::DeadlineAsc,
                TaskCursor::DeadlineAsc {
                    soft_deadline: None,
                    id,
                },
            ) => Condition::all()
                .add(tasks::Column::SoftDeadline.is_null())
                .add(tasks::Column::Id.lt(id)),
            _ => return Err(AppError::BadRequest),
        })
    }
}

async fn build_task_detail_response(
    state: &AppState,
    project_id: Uuid,
    task: tasks::Model,
) -> Result<TaskDetailResponse, AppError> {
    let custom_field_values = load_task_custom_field_values(&state.db, project_id, task.id).await?;
    Ok(TaskDetailResponse {
        task: build_task_response(&state.db, task).await?,
        custom_field_values,
    })
}

// ─── Tasks ───────────────────────────────────────────────────────────────

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/",
    tag = "Tasks",
    summary = "タスク一覧",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ListTasksQuery,
    ),
    responses(
        (status = 200, description = "タスク一覧", body = TaskListResponse),
        CrudErrors,
    )
)]
pub async fn list_tasks(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id)): Path<(Uuid, Uuid)>,
    Query(q): Query<ListTasksQuery>,
) -> Result<Json<TaskListResponse>, AppError> {
    auth.require_scope(entity::scopes::Scope::ReadTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;

    let mut query = tasks::Entity::find()
        .filter(tasks::Column::ProjectId.eq(project_id))
        .filter(tasks::Column::DeletedAt.is_null())
        .filter(tasks::Column::IsArchived.eq(q.is_archived));

    if let Some(sid) = q.status_id {
        query = query.filter(tasks::Column::StatusId.eq(sid));
    }
    if let Some(ref priority) = q.priority {
        let priority = parse_task_priority(priority)?;
        query = query.filter(tasks::Column::Priority.eq(priority));
    }
    if let Some(mid) = q.milestone_id {
        query = query.filter(tasks::Column::MilestoneId.eq(mid));
    }
    if let Some(sid) = q.sprint_id {
        query = query.filter(tasks::Column::SprintId.eq(sid));
    }
    if let Some(pid) = q.parent_task_id {
        query = query.filter(tasks::Column::ParentTaskId.eq(pid));
    }
    if let Some(uid) = q.assignee_id {
        query = query.filter(Expr::cust_with_values(
            "EXISTS (SELECT 1 FROM task_assignees WHERE task_assignees.task_id = tasks.id AND task_assignees.user_id = $1)",
            vec![sea_orm::Value::from(uid)],
        ));
    }
    if let Some(lid) = q.label_id {
        query = query.filter(Expr::cust_with_values(
            "EXISTS (SELECT 1 FROM task_labels WHERE task_labels.task_id = tasks.id AND task_labels.label_id = $1)",
            vec![sea_orm::Value::from(lid)],
        ));
    }

    let sort = TaskSort::parse(q.sort.as_deref());
    let limit = q.limit.clamp(1, 200);
    let total = query.clone().count(&state.db).await?;

    // カーソルは並びのキーを持ち回るので、`offset` と混ぜると起点が二重になる。
    // 黙って片方を捨てると呼び出し側の間違いが表に出ないので弾く
    if q.cursor.is_some() && q.offset != 0 {
        return Err(AppError::BadRequest);
    }
    if let Some(raw) = &q.cursor {
        query = query.filter(sort.keyset_after(decode_cursor(raw)?)?);
    }

    query = match sort {
        TaskSort::PriorityAsc => query.order_by_asc(tasks::Column::Priority),
        TaskSort::DeadlineAsc => query.order_by_asc(tasks::Column::SoftDeadline),
        TaskSort::CreatedAtDesc => query.order_by_desc(tasks::Column::CreatedAt),
    };
    // 同じ値を持つ行の順序は Postgres 上で未定義。タイブレーカーが無いと、同じデータでも
    // ページ境界に同値の行があるだけで重複・欠落が出る。カーソルの不等式もこの並びと
    // 同じ形にする（priority / deadline は同値がもっと多い）
    query = query.order_by_desc(tasks::Column::Id);

    // 「まだ残っているか」は総数との比較ではなく 1 件多く引いて確かめる。
    // 総数は取得中に動くので、比較では終わらない「もっと見る」が残る
    let mut rows = query
        .offset(q.offset)
        .limit(limit + 1)
        .all(&state.db)
        .await?;
    let has_more = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    let next_cursor = if has_more {
        rows.last().map(|task| sort.cursor_of(task))
    } else {
        None
    };
    Ok(Json(TaskListResponse {
        tasks: build_task_responses(&state.db, rows).await?,
        total,
        next_cursor,
    }))
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/",
    tag = "Tasks",
    summary = "タスク作成",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
    ),
    request_body = CreateTaskRequest,
    responses(
        (status = 201, description = "作成されたタスク", body = TaskDetailResponse),
        CrudErrors,
    )
)]
pub async fn create_task(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id)): Path<(Uuid, Uuid)>,
    Valid(Json(payload)): Valid<Json<CreateTaskRequest>>,
) -> Result<(StatusCode, Json<TaskDetailResponse>), AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;

    if let (Some(s), Some(h)) = (payload.soft_deadline, payload.hard_deadline)
        && s >= h
    {
        return Err(AppError::BadRequest);
    }

    let txn = state.db.begin().await?;
    let status = project_statuses::Entity::find_by_id(payload.status_id)
        .filter(project_statuses::Column::ProjectId.eq(project_id))
        .one(&txn)
        .await?
        .ok_or(AppError::NotFound)?;

    // parent_task_id / milestone_id が同一プロジェクトに属することを検証
    if let Some(pid) = payload.parent_task_id {
        tasks::Entity::find_by_id(pid)
            .filter(tasks::Column::ProjectId.eq(project_id))
            .filter(tasks::Column::DeletedAt.is_null())
            .one(&txn)
            .await?
            .ok_or(AppError::NotFound)?;
    }
    if let Some(mid) = payload.milestone_id {
        milestones::Entity::find_by_id(mid)
            .filter(milestones::Column::ProjectId.eq(project_id))
            .one(&txn)
            .await?
            .ok_or(AppError::NotFound)?;
    }
    if let Some(sid) = payload.sprint_id {
        let sprint = sprints::Entity::find_by_id(sid)
            .filter(sprints::Column::ProjectId.eq(project_id))
            .lock(LockType::Update)
            .one(&txn)
            .await?
            .ok_or(AppError::NotFound)?;
        if sprint.status == sprints::SprintStatus::Completed {
            return Err(AppError::Conflict);
        }
    }

    let seq_id = service::tasks::next_seq_id(&txn, project_id).await?;

    let model = tasks::ActiveModel {
        id: Set(Uuid::new_v4()),
        project_id: Set(project_id),
        seq_id: Set(seq_id),
        title: Set(payload.title),
        description: Set(payload.description),
        status_id: Set(payload.status_id),
        priority: Set(payload.priority.unwrap_or(tasks::TaskPriority::Medium)),
        progress_pct: Set(payload.progress_pct.unwrap_or(0)),
        parent_task_id: Set(payload.parent_task_id),
        milestone_id: Set(payload.milestone_id),
        sprint_id: Set(payload.sprint_id),
        soft_deadline: Set(payload.soft_deadline.map(Into::into)),
        hard_deadline: Set(payload.hard_deadline.map(Into::into)),
        estimated_minutes: Set(payload.estimated_minutes),
        is_archived: Set(false),
        created_by: Set(auth.user_id),
        created_at: Set(chrono::Utc::now().into()),
        updated_at: Set(chrono::Utc::now().into()),
        completed_at: Set(status.is_done_state.then(|| chrono::Utc::now().into())),
        deleted_at: Set(None),
    }
    .insert(&txn)
    .await?;

    for a in &payload.assignees {
        require_project_access(&state, tenant_id, project_id, a.user_id).await?;
        task_assignees::ActiveModel {
            id: Set(Uuid::new_v4()),
            task_id: Set(model.id),
            user_id: Set(a.user_id),
            role: Set(a.role.clone()),
            assigned_at: Set(chrono::Utc::now().into()),
        }
        .insert(&txn)
        .await?;
        notify_assigned(&txn, project_id, model.id, a.user_id, auth.user_id, &a.role).await?;
    }
    let unique_label_ids: Vec<Uuid> = {
        let mut v = payload.label_ids.clone();
        v.sort();
        v.dedup();
        v
    };
    if !unique_label_ids.is_empty() {
        let labels_in_project = labels::Entity::find()
            .filter(labels::Column::Id.is_in(unique_label_ids.clone()))
            .filter(labels::Column::ProjectId.eq(project_id))
            .all(&txn)
            .await?;
        if labels_in_project.len() != unique_label_ids.len() {
            return Err(AppError::BadRequest);
        }
    }
    for lid in &unique_label_ids {
        task_labels::ActiveModel {
            task_id: Set(model.id),
            label_id: Set(*lid),
        }
        .insert(&txn)
        .await?;
    }

    if status.is_done_state {
        ensure_required_custom_fields(
            &txn,
            project_id,
            model.id,
            Some(&payload.custom_field_values),
        )
        .await?;
    }
    upsert_task_custom_field_values(&txn, project_id, model.id, &payload.custom_field_values)
        .await?;

    record_activity(
        &txn,
        model.id,
        Some(auth.user_id),
        "task_created",
        serde_json::json!({}),
    )
    .await?;
    txn.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(build_task_detail_response(&state, project_id, model).await?),
    ))
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "Tasks",
    summary = "タスク取得",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = String, Path, description = "タスクID（UUID または ENG-42 形式）"),
    ),
    responses(
        (status = 200, description = "タスク", body = TaskDetailResponse),
        CrudErrors,
    )
)]
pub async fn get_task(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id)): Path<(Uuid, Uuid, String)>,
) -> Result<Json<TaskDetailResponse>, AppError> {
    auth.require_scope(entity::scopes::Scope::ReadTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let task = resolve_task(&state, tenant_id, project_id, &id).await?;
    Ok(Json(
        build_task_detail_response(&state, project_id, task).await?,
    ))
}

#[axum::debug_handler]
#[utoipa::path(
    put,
    path = "/{id}",
    tag = "Tasks",
    summary = "タスク更新",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = String, Path, description = "タスクID"),
    ),
    request_body = UpdateTaskRequest,
    responses(
        (status = 200, description = "更新後のタスク", body = TaskDetailResponse),
        CrudErrors,
    )
)]
pub async fn update_task(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id)): Path<(Uuid, Uuid, String)>,
    Valid(Json(payload)): Valid<Json<UpdateTaskRequest>>,
) -> Result<Json<TaskDetailResponse>, AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let task = resolve_task(&state, tenant_id, project_id, &id).await?;
    let task_snapshot = task.clone();
    let task_id = task.id;
    let existing_soft = task.soft_deadline.map(|dt| dt.with_timezone(&Utc));
    let existing_hard = task.hard_deadline.map(|dt| dt.with_timezone(&Utc));
    let parent_changes = payload.clear_parent_task_id || payload.parent_task_id.is_some();
    let txn = if parent_changes {
        state
            .db
            .begin_with_config(Some(IsolationLevel::Serializable), None)
            .await?
    } else {
        state.db.begin().await?
    };

    let mut active: tasks::ActiveModel = task.into();
    if let Some(ref v) = payload.title {
        active.title = Set(v.clone());
    }
    if payload.clear_description {
        active.description = Set(None);
    } else if let Some(ref v) = payload.description {
        active.description = Set(Some(v.clone()));
    }
    if let Some(v) = payload.status_id {
        let status = project_statuses::Entity::find_by_id(v)
            .filter(project_statuses::Column::ProjectId.eq(project_id))
            .one(&txn)
            .await?
            .ok_or(AppError::NotFound)?;
        if status.is_done_state {
            ensure_required_custom_fields(
                &txn,
                project_id,
                task_id,
                payload.custom_field_values.as_deref(),
            )
            .await?;
        }
        active.completed_at = if status.is_done_state {
            match active.completed_at.clone() {
                sea_orm::ActiveValue::Set(Some(completed_at))
                | sea_orm::ActiveValue::Unchanged(Some(completed_at)) => Set(Some(completed_at)),
                _ => Set(Some(chrono::Utc::now().into())),
            }
        } else {
            Set(None)
        };
        active.status_id = Set(v);
    }
    if let Some(v) = payload.priority {
        active.priority = Set(v);
    }
    if let Some(v) = payload.progress_pct {
        active.progress_pct = Set(v);
    }
    if payload.clear_milestone_id {
        active.milestone_id = Set(None);
    } else if let Some(v) = payload.milestone_id {
        milestones::Entity::find_by_id(v)
            .filter(milestones::Column::ProjectId.eq(project_id))
            .one(&txn)
            .await?
            .ok_or(AppError::NotFound)?;
        active.milestone_id = Set(Some(v));
    }
    if payload.clear_sprint_id {
        active.sprint_id = Set(None);
    } else if let Some(v) = payload.sprint_id {
        let sprint = sprints::Entity::find_by_id(v)
            .filter(sprints::Column::ProjectId.eq(project_id))
            .lock(LockType::Update)
            .one(&txn)
            .await?
            .ok_or(AppError::NotFound)?;
        if sprint.status == sprints::SprintStatus::Completed {
            return Err(AppError::Conflict);
        }
        active.sprint_id = Set(Some(v));
    }
    if payload.clear_soft_deadline {
        active.soft_deadline = Set(None);
    } else if let Some(v) = payload.soft_deadline {
        active.soft_deadline = Set(Some(v.into()));
    }
    if payload.clear_hard_deadline {
        active.hard_deadline = Set(None);
    } else if let Some(v) = payload.hard_deadline {
        active.hard_deadline = Set(Some(v.into()));
    }

    let effective_soft = if payload.clear_soft_deadline {
        None
    } else if let Some(v) = payload.soft_deadline {
        Some(v)
    } else {
        existing_soft
    };
    let effective_hard = if payload.clear_hard_deadline {
        None
    } else if let Some(v) = payload.hard_deadline {
        Some(v)
    } else {
        existing_hard
    };
    if let (Some(s), Some(h)) = (effective_soft, effective_hard)
        && s >= h
    {
        return Err(AppError::BadRequest);
    }

    if payload.clear_estimated_minutes {
        active.estimated_minutes = Set(None);
    } else if let Some(v) = payload.estimated_minutes {
        active.estimated_minutes = Set(Some(v));
    }
    if let Some(v) = payload.is_archived {
        active.is_archived = Set(v);
    }
    active.updated_at = Set(chrono::Utc::now().into());

    if let Some(ref label_ids) = payload.label_ids {
        // task_labels が空でも同じタスクへのラベル置換を直列化するため、
        // 関連行ではなく必ず存在する親タスク行を共通のロック対象にする。
        // 一括更新もラベル操作前の tasks UPDATE で同じ行ロックを取る。
        tasks::Entity::find_by_id(task_id)
            .filter(tasks::Column::ProjectId.eq(project_id))
            .filter(tasks::Column::DeletedAt.is_null())
            .lock(LockType::Update)
            .one(&txn)
            .await?
            .ok_or(AppError::NotFound)?;

        let mut unique = label_ids.clone();
        unique.sort();
        unique.dedup();
        if !unique.is_empty() {
            let in_project = labels::Entity::find()
                .filter(labels::Column::Id.is_in(unique.clone()))
                .filter(labels::Column::ProjectId.eq(project_id))
                .all(&txn)
                .await?;
            if in_project.len() != unique.len() {
                return Err(AppError::BadRequest);
            }
        }
        let before_labels = task_label_entries(&txn, task_id).await?;
        task_labels::Entity::delete_many()
            .filter(task_labels::Column::TaskId.eq(task_id))
            .exec(&txn)
            .await?;
        for lid in unique {
            task_labels::ActiveModel {
                task_id: Set(task_id),
                label_id: Set(lid),
            }
            .insert(&txn)
            .await?;
        }
        let after_labels = task_label_entries(&txn, task_id).await?;
        record_label_diff(
            &txn,
            task_id,
            Some(auth.user_id),
            &before_labels,
            &after_labels,
        )
        .await?;
    }

    if parent_changes {
        let fresh = tasks::Entity::find_by_id(task_id)
            .filter(tasks::Column::ProjectId.eq(project_id))
            .filter(tasks::Column::DeletedAt.is_null())
            .one(&txn)
            .await?
            .ok_or(AppError::NotFound)?;

        if payload.clear_parent_task_id {
            active.parent_task_id = Set(None);
        } else if let Some(new_parent_id) = payload.parent_task_id {
            if new_parent_id == fresh.id {
                return Err(AppError::Conflict);
            }
            if is_ancestor_of(&txn, project_id, fresh.id, new_parent_id).await? {
                return Err(AppError::Conflict);
            }
            tasks::Entity::find_by_id(new_parent_id)
                .filter(tasks::Column::ProjectId.eq(project_id))
                .filter(tasks::Column::DeletedAt.is_null())
                .one(&txn)
                .await?
                .ok_or(AppError::NotFound)?;
            active.parent_task_id = Set(Some(new_parent_id));
        }

        let updated = active.update(&txn).await?;
        record_task_field_activities(
            &txn,
            task_id,
            auth.user_id,
            project_id,
            &task_snapshot,
            &payload,
        )
        .await?;
        if let Some(ref values) = payload.custom_field_values {
            upsert_task_custom_field_values(&txn, project_id, task_id, values).await?;
        }
        let linked = service::github::sync::mark_pending_push(&txn, task_id).await?;
        txn.commit().await?;
        if linked {
            crate::handlers::github::enqueue_issue_push(&state, task_id).await;
        }
        Ok(Json(
            build_task_detail_response(&state, project_id, updated).await?,
        ))
    } else {
        let updated = active.update(&txn).await?;
        record_task_field_activities(
            &txn,
            task_id,
            auth.user_id,
            project_id,
            &task_snapshot,
            &payload,
        )
        .await?;
        if let Some(ref values) = payload.custom_field_values {
            upsert_task_custom_field_values(&txn, project_id, task_id, values).await?;
        }
        let linked = service::github::sync::mark_pending_push(&txn, task_id).await?;
        txn.commit().await?;
        if linked {
            crate::handlers::github::enqueue_issue_push(&state, task_id).await;
        }
        Ok(Json(
            build_task_detail_response(&state, project_id, updated).await?,
        ))
    }
}

#[axum::debug_handler]
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "Tasks",
    summary = "タスク削除（ソフト）",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = String, Path, description = "タスクID"),
    ),
    responses(
        (status = 204, description = "削除しました"),
        CrudErrors,
    )
)]
pub async fn delete_task(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id)): Path<(Uuid, Uuid, String)>,
) -> Result<StatusCode, AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let task = resolve_task(&state, tenant_id, project_id, &id).await?;
    if task.created_by != auth.user_id
        && !is_tenant_owner(&state.db, tenant_id, auth.user_id).await?
    {
        return Err(AppError::Forbidden);
    }
    let mut active: tasks::ActiveModel = task.into();
    active.deleted_at = Set(Some(chrono::Utc::now().into()));
    active.update(&state.db).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/{id}/archive",
    tag = "Tasks",
    summary = "タスクをアーカイブ",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = String, Path, description = "タスクID"),
    ),
    responses(
        (status = 200, description = "アーカイブ後のタスク", body = TaskResponse),
        CrudErrors,
    )
)]
pub async fn archive_task(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id)): Path<(Uuid, Uuid, String)>,
) -> Result<Json<TaskResponse>, AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let task = resolve_task(&state, tenant_id, project_id, &id).await?;
    let task_id = task.id;
    let txn = state.db.begin().await?;
    let mut active: tasks::ActiveModel = task.into();
    active.is_archived = Set(true);
    active.updated_at = Set(chrono::Utc::now().into());
    let updated = active.update(&txn).await?;
    record_activity(
        &txn,
        task_id,
        Some(auth.user_id),
        "archived",
        serde_json::json!({}),
    )
    .await?;
    txn.commit().await?;
    Ok(Json(build_task_response(&state.db, updated).await?))
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/{id}/unarchive",
    tag = "Tasks",
    summary = "タスクのアーカイブ解除",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = String, Path, description = "タスクID"),
    ),
    responses(
        (status = 200, description = "アーカイブ解除後のタスク", body = TaskResponse),
        CrudErrors,
    )
)]
pub async fn unarchive_task(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id)): Path<(Uuid, Uuid, String)>,
) -> Result<Json<TaskResponse>, AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let task = resolve_task(&state, tenant_id, project_id, &id).await?;
    let task_id = task.id;
    let txn = state.db.begin().await?;
    let mut active: tasks::ActiveModel = task.into();
    active.is_archived = Set(false);
    active.updated_at = Set(chrono::Utc::now().into());
    let updated = active.update(&txn).await?;
    record_activity(
        &txn,
        task_id,
        Some(auth.user_id),
        "unarchived",
        serde_json::json!({}),
    )
    .await?;
    txn.commit().await?;
    Ok(Json(build_task_response(&state.db, updated).await?))
}

// ─── Assignees ───────────────────────────────────────────────────────────

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/{id}/assignees",
    tag = "Tasks",
    summary = "担当者一覧",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = String, Path, description = "タスクID"),
    ),
    responses(
        (status = 200, description = "担当者一覧", body = [TaskAssigneeResponse]),
        CrudErrors,
    )
)]
pub async fn list_assignees(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id)): Path<(Uuid, Uuid, String)>,
) -> Result<Json<Vec<TaskAssigneeResponse>>, AppError> {
    auth.require_scope(entity::scopes::Scope::ReadTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let task = resolve_task(&state, tenant_id, project_id, &id).await?;
    let assignees = task_assignees::Entity::find()
        .filter(task_assignees::Column::TaskId.eq(task.id))
        .all(&state.db)
        .await?;
    Ok(Json(assignees.into_iter().map(Into::into).collect()))
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/{id}/assignees",
    tag = "Tasks",
    summary = "担当者追加",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = String, Path, description = "タスクID"),
    ),
    request_body = AddAssigneeRequest,
    responses(
        (status = 201, description = "追加された担当者", body = TaskAssigneeResponse),
        CrudErrors,
    )
)]
pub async fn add_assignee(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id)): Path<(Uuid, Uuid, String)>,
    Valid(Json(payload)): Valid<Json<AddAssigneeRequest>>,
) -> Result<(StatusCode, Json<TaskAssigneeResponse>), AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let task = resolve_task(&state, tenant_id, project_id, &id).await?;
    require_project_access(&state, tenant_id, project_id, payload.user_id).await?;
    let duplicate = task_assignees::Entity::find()
        .filter(task_assignees::Column::TaskId.eq(task.id))
        .filter(task_assignees::Column::UserId.eq(payload.user_id))
        .one(&state.db)
        .await?;
    if duplicate.is_some() {
        return Err(AppError::Conflict);
    }
    let role = payload.role.clone();
    let txn = state.db.begin().await?;
    let assignee = task_assignees::ActiveModel {
        id: Set(Uuid::new_v4()),
        task_id: Set(task.id),
        user_id: Set(payload.user_id),
        role: Set(role.clone()),
        assigned_at: Set(chrono::Utc::now().into()),
    }
    .insert(&txn)
    .await?;
    record_activity(
        &txn,
        task.id,
        Some(auth.user_id),
        "assignee_added",
        serde_json::json!({
            "user_id": payload.user_id,
            "role": role,
        }),
    )
    .await?;
    notify_assigned(
        &txn,
        project_id,
        task.id,
        payload.user_id,
        auth.user_id,
        &role,
    )
    .await?;
    txn.commit().await?;
    Ok((StatusCode::CREATED, Json(assignee.into())))
}

#[axum::debug_handler]
#[utoipa::path(
    put,
    path = "/{id}/assignees/{user_id}",
    tag = "Tasks",
    summary = "担当者ロール変更",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = String, Path, description = "タスクID"),
        ("user_id" = Uuid, Path, description = "ユーザーID"),
    ),
    request_body = UpdateAssigneeRequest,
    responses(
        (status = 200, description = "更新後の担当者", body = TaskAssigneeResponse),
        CrudErrors,
    )
)]
pub async fn update_assignee(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id, user_id)): Path<(Uuid, Uuid, String, Uuid)>,
    Valid(Json(payload)): Valid<Json<UpdateAssigneeRequest>>,
) -> Result<Json<TaskAssigneeResponse>, AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let task = resolve_task(&state, tenant_id, project_id, &id).await?;
    let assignee = task_assignees::Entity::find()
        .filter(task_assignees::Column::TaskId.eq(task.id))
        .filter(task_assignees::Column::UserId.eq(user_id))
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    let mut active: task_assignees::ActiveModel = assignee.into();
    active.role = Set(payload.role);
    Ok(Json(active.update(&state.db).await?.into()))
}

#[axum::debug_handler]
#[utoipa::path(
    delete,
    path = "/{id}/assignees/{user_id}",
    tag = "Tasks",
    summary = "担当者削除",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = String, Path, description = "タスクID"),
        ("user_id" = Uuid, Path, description = "ユーザーID"),
    ),
    responses(
        (status = 204, description = "削除しました"),
        CrudErrors,
    )
)]
pub async fn remove_assignee(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id, user_id)): Path<(Uuid, Uuid, String, Uuid)>,
) -> Result<StatusCode, AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let task = resolve_task(&state, tenant_id, project_id, &id).await?;
    let assignee = task_assignees::Entity::find()
        .filter(task_assignees::Column::TaskId.eq(task.id))
        .filter(task_assignees::Column::UserId.eq(user_id))
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    let txn = state.db.begin().await?;
    task_assignees::Entity::delete_by_id(assignee.id)
        .exec(&txn)
        .await?;
    record_activity(
        &txn,
        task.id,
        Some(auth.user_id),
        "assignee_removed",
        serde_json::json!({ "user_id": user_id }),
    )
    .await?;
    txn.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── Relations ───────────────────────────────────────────────────────────

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/{id}/relations",
    tag = "Tasks",
    summary = "依存関係一覧",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = String, Path, description = "タスクID"),
    ),
    responses(
        (status = 200, description = "依存関係", body = TaskRelationsResponse),
        CrudErrors,
    )
)]
pub async fn list_relations(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id)): Path<(Uuid, Uuid, String)>,
) -> Result<Json<TaskRelationsResponse>, AppError> {
    auth.require_scope(entity::scopes::Scope::ReadTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let task = resolve_task(&state, tenant_id, project_id, &id).await?;

    let subtasks = tasks::Entity::find()
        .filter(tasks::Column::ParentTaskId.eq(task.id))
        .filter(tasks::Column::DeletedAt.is_null())
        .all(&state.db)
        .await?;

    let blocks_rels = task_relations::Entity::find()
        .filter(task_relations::Column::BlockerTaskId.eq(task.id))
        .all(&state.db)
        .await?;
    let blocked_ids: Vec<Uuid> = blocks_rels.iter().map(|r| r.blocked_task_id).collect();
    let blocked_models = if blocked_ids.is_empty() {
        vec![]
    } else {
        tasks::Entity::find()
            .filter(tasks::Column::Id.is_in(blocked_ids))
            .filter(tasks::Column::ProjectId.eq(project_id))
            .filter(tasks::Column::DeletedAt.is_null())
            .all(&state.db)
            .await?
    };
    let blocked_tasks: HashMap<Uuid, TaskResponse> =
        build_task_responses(&state.db, blocked_models)
            .await?
            .into_iter()
            .map(|t| (t.id, t))
            .collect();
    let blocks: Vec<RelationEntry> = blocks_rels
        .into_iter()
        .filter_map(|rel| {
            blocked_tasks
                .get(&rel.blocked_task_id)
                .map(|t| RelationEntry {
                    relation_id: rel.id,
                    task: t.clone(),
                })
        })
        .collect();

    let blocked_rels = task_relations::Entity::find()
        .filter(task_relations::Column::BlockedTaskId.eq(task.id))
        .all(&state.db)
        .await?;
    let blocker_ids: Vec<Uuid> = blocked_rels.iter().map(|r| r.blocker_task_id).collect();
    let blocker_models = if blocker_ids.is_empty() {
        vec![]
    } else {
        tasks::Entity::find()
            .filter(tasks::Column::Id.is_in(blocker_ids))
            .filter(tasks::Column::ProjectId.eq(project_id))
            .filter(tasks::Column::DeletedAt.is_null())
            .all(&state.db)
            .await?
    };
    let blocker_tasks: HashMap<Uuid, TaskResponse> =
        build_task_responses(&state.db, blocker_models)
            .await?
            .into_iter()
            .map(|t| (t.id, t))
            .collect();
    let blocked_by: Vec<RelationEntry> = blocked_rels
        .into_iter()
        .filter_map(|rel| {
            blocker_tasks
                .get(&rel.blocker_task_id)
                .map(|t| RelationEntry {
                    relation_id: rel.id,
                    task: t.clone(),
                })
        })
        .collect();

    Ok(Json(TaskRelationsResponse {
        subtasks: build_task_responses(&state.db, subtasks).await?,
        blocks,
        blocked_by,
    }))
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/{id}/relations",
    tag = "Tasks",
    summary = "依存関係追加（循環検出あり）",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = String, Path, description = "タスクID"),
    ),
    request_body = AddRelationRequest,
    responses(
        (status = 201, description = "追加された依存関係", body = TaskRelationResponse),
        CrudErrors,
    )
)]
pub async fn add_relation(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id)): Path<(Uuid, Uuid, String)>,
    Json(payload): Json<AddRelationRequest>,
) -> Result<(StatusCode, Json<TaskRelationResponse>), AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let task = resolve_task(&state, tenant_id, project_id, &id).await?;
    resolve_task(
        &state,
        tenant_id,
        project_id,
        &payload.target_task_id.to_string(),
    )
    .await?;

    let (blocker, blocked) = match payload.relation_type.as_str() {
        "blocks" => (task.id, payload.target_task_id),
        "blocked_by" => (payload.target_task_id, task.id),
        _ => return Err(AppError::BadRequest),
    };

    // SERIALIZABLE: prevent concurrent inverse relations from bypassing cycle detection
    // (READ COMMITTED allows both T1/T2 to pass would_create_cycle before either commits).
    let txn = state
        .db
        .begin_with_config(Some(IsolationLevel::Serializable), None)
        .await?;
    if would_create_cycle(&txn, project_id, blocker, blocked).await? {
        return Err(AppError::Conflict);
    }

    let rel = match (task_relations::ActiveModel {
        id: Set(Uuid::new_v4()),
        blocker_task_id: Set(blocker),
        blocked_task_id: Set(blocked),
        created_at: Set(chrono::Utc::now().into()),
    }
    .insert(&txn)
    .await)
    {
        Ok(rel) => rel,
        Err(e) if is_postgres_unique_violation(&e) => return Err(AppError::Conflict),
        Err(e) => return Err(e.into()),
    };
    let target_task = tasks::Entity::find_by_id(payload.target_task_id)
        .filter(tasks::Column::ProjectId.eq(project_id))
        .one(&txn)
        .await?
        .ok_or(AppError::NotFound)?;
    let relation_type = if payload.relation_type == "blocked_by" {
        "blocks"
    } else {
        payload.relation_type.as_str()
    };
    record_activity(
        &txn,
        task.id,
        Some(auth.user_id),
        "relation_added",
        serde_json::json!({
            "type": relation_type,
            "target_seq_id": target_task.seq_id,
        }),
    )
    .await?;
    txn.commit().await?;

    Ok((StatusCode::CREATED, Json(rel.into())))
}

#[axum::debug_handler]
#[utoipa::path(
    delete,
    path = "/{id}/relations/{relation_id}",
    tag = "Tasks",
    summary = "依存関係削除",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("project_id" = Uuid, Path, description = "プロジェクトID"),
        ("id" = String, Path, description = "タスクID"),
        ("relation_id" = Uuid, Path, description = "依存関係ID"),
    ),
    responses(
        (status = 204, description = "削除しました"),
        CrudErrors,
    )
)]
pub async fn remove_relation(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, project_id, id, relation_id)): Path<(Uuid, Uuid, String, Uuid)>,
) -> Result<StatusCode, AppError> {
    auth.require_scope(entity::scopes::Scope::WriteTask)?;
    auth.ensure_tenant_access(&state, tenant_id, Some(project_id))
        .await?;
    let task = resolve_task(&state, tenant_id, project_id, &id).await?;
    let rel = task_relations::Entity::find_by_id(relation_id)
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    if rel.blocker_task_id != task.id && rel.blocked_task_id != task.id {
        return Err(AppError::NotFound);
    }
    let other_task_id = if rel.blocker_task_id == task.id {
        rel.blocked_task_id
    } else {
        rel.blocker_task_id
    };
    let other_task = tasks::Entity::find_by_id(other_task_id)
        .filter(tasks::Column::ProjectId.eq(project_id))
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    let relation_type = if rel.blocker_task_id == task.id {
        "blocks"
    } else {
        "blocked_by"
    };
    let txn = state.db.begin().await?;
    task_relations::Entity::delete_by_id(relation_id)
        .exec(&txn)
        .await?;
    record_activity(
        &txn,
        task.id,
        Some(auth.user_id),
        "relation_removed",
        serde_json::json!({
            "type": relation_type,
            "target_seq_id": other_task.seq_id,
        }),
    )
    .await?;
    txn.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
