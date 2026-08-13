use std::collections::{HashMap, HashSet};

use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, prelude::Uuid};

use crate::error::AppError;
use entity::{labels, task_assignees, task_labels, tasks, users};
use payload::labels::LabelResponse;
use payload::tasks::{TaskAssigneeSummary, TaskResponse};
use payload::users::UserSummary;

/// tasks::Model の集合からユーザー情報（作成者・担当者）とラベルを埋め込んだ
/// TaskResponse を組み立てる。関連はバッチ取得（追加クエリ4本）。
pub async fn build_task_responses<C: ConnectionTrait>(
    db: &C,
    task_models: Vec<tasks::Model>,
) -> Result<Vec<TaskResponse>, AppError> {
    if task_models.is_empty() {
        return Ok(Vec::new());
    }

    let task_ids: Vec<Uuid> = task_models.iter().map(|t| t.id).collect();
    let assignee_rows = task_assignees::Entity::find()
        .filter(task_assignees::Column::TaskId.is_in(task_ids.clone()))
        .all(db)
        .await?;

    let task_label_rows = task_labels::Entity::find()
        .filter(task_labels::Column::TaskId.is_in(task_ids))
        .all(db)
        .await?;
    let label_map: HashMap<Uuid, LabelResponse> = labels::Entity::find()
        .filter(labels::Column::Id.is_in(task_label_rows.iter().map(|tl| tl.label_id)))
        .all(db)
        .await?
        .into_iter()
        .map(|l| (l.id, l.into()))
        .collect();
    let mut labels_by_task: HashMap<Uuid, Vec<LabelResponse>> = HashMap::new();
    for tl in task_label_rows {
        if let Some(label) = label_map.get(&tl.label_id) {
            labels_by_task
                .entry(tl.task_id)
                .or_default()
                .push(label.clone());
        }
    }

    let mut user_ids: HashSet<Uuid> = task_models.iter().map(|t| t.created_by).collect();
    user_ids.extend(assignee_rows.iter().map(|a| a.user_id));
    let user_map: HashMap<Uuid, UserSummary> = users::Entity::find()
        .filter(users::Column::Id.is_in(user_ids))
        .all(db)
        .await?
        .into_iter()
        .map(|u| (u.id, u.into()))
        .collect();

    let mut assignees_by_task: HashMap<Uuid, Vec<TaskAssigneeSummary>> = HashMap::new();
    for a in assignee_rows {
        if let Some(user) = user_map.get(&a.user_id) {
            assignees_by_task
                .entry(a.task_id)
                .or_default()
                .push(TaskAssigneeSummary {
                    role: a.role,
                    user: user.clone(),
                });
        }
    }

    Ok(task_models
        .into_iter()
        .map(|t| {
            // created_by は users への FK なので通常必ず解決できるが、FK 制約外の経路
            // （直接 DB 操作等）で作成者行だけ欠損しても一覧全体を落とさず null に
            // 縮退する（担当者欠損のスキップと同方針で可用性を優先）。
            let created_by = user_map.get(&t.created_by).cloned();
            let assignees = assignees_by_task.remove(&t.id).unwrap_or_default();
            let task_labels = labels_by_task.remove(&t.id).unwrap_or_default();
            TaskResponse::from_parts(t, created_by, assignees, task_labels)
        })
        .collect())
}

pub async fn build_task_response<C: ConnectionTrait>(
    db: &C,
    task: tasks::Model,
) -> Result<TaskResponse, AppError> {
    let mut responses = build_task_responses(db, vec![task]).await?;
    responses
        .pop()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("empty task response batch")))
}
