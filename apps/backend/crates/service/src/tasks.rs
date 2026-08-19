//! タスクの共通ロジック。

use sea_orm::sea_query::LockType;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ConnectionTrait, EntityTrait, QuerySelect, prelude::Uuid,
};

use entity::project_task_counters;

/// プロジェクト内の次の連番を採番する。
/// 行ロックで直列化するため、必ずトランザクション上で呼ぶこと。
pub async fn next_seq_id<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
) -> Result<i32, sea_orm::DbErr> {
    let existing = project_task_counters::Entity::find_by_id(project_id)
        .lock(LockType::Update)
        .one(db)
        .await?;
    Ok(match existing {
        Some(c) => {
            let new_seq = c.last_seq + 1;
            let mut active: project_task_counters::ActiveModel = c.into();
            active.last_seq = Set(new_seq);
            active.update(db).await?.last_seq
        }
        None => {
            project_task_counters::ActiveModel {
                project_id: Set(project_id),
                last_seq: Set(1),
            }
            .insert(db)
            .await?
            .last_seq
        }
    })
}
