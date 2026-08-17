//! テナント / プロジェクトのアクセス判定（#568）。
//!
//! 認可（handler）と通知の宛先抽出（service）が同じルールを見る必要があるため、
//! ここに一本化する。handler 側は `auth_helpers` が再公開している。

use std::collections::HashSet;

use sea_orm::{
    ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter, QuerySelect,
    prelude::Uuid,
};

use common::error::AppError;
use entity::{project_members, projects, tenant_members};

/// テナントメンバーかどうか（オーナーは含まない）。
pub async fn is_tenant_member<C: ConnectionTrait>(
    db: &C,
    tenant_id: Uuid,
    user_id: Uuid,
) -> Result<bool, AppError> {
    Ok(tenant_members::Entity::find()
        .filter(tenant_members::Column::TenantId.eq(tenant_id))
        .filter(tenant_members::Column::UserId.eq(user_id))
        .one(db)
        .await?
        .is_some())
}

/// プロジェクト単位のアクセス可否。**テナントに入れることは呼び出し側で確認済みの前提。**
///
/// メンバーを 1 人も指定していないプロジェクトはテナント全体に開放し、
/// 指定がある場合だけその中に居るかを見る（#568）。
///
/// 個人プロジェクト（Inbox）は作成時に本人が `project_members` に入る
/// （`my_tasks::seed_personal_project_defaults`）ので、ここで開放されることはない。
pub async fn project_is_open_or_member<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    user_id: Uuid,
) -> Result<bool, AppError> {
    // 自分が指定されていればそこで確定（指定あり側の判定を 1 クエリで終わらせる）
    if project_members::Entity::find()
        .filter(project_members::Column::ProjectId.eq(project_id))
        .filter(project_members::Column::UserId.eq(user_id))
        .one(db)
        .await?
        .is_some()
    {
        return Ok(true);
    }

    Ok(project_members::Entity::find()
        .filter(project_members::Column::ProjectId.eq(project_id))
        .count(db)
        .await?
        == 0)
}

/// 候補プロジェクトのうち、そのユーザーに見えるものを返す。
///
/// `project_is_open_or_member` をプロジェクトごとに呼ぶと件数分のクエリが出るため、
/// 一覧系ではこちらを使う。**テナントに入れることは呼び出し側で確認済みの前提。**
pub async fn visible_project_ids<C: ConnectionTrait>(
    db: &C,
    candidate_ids: Vec<Uuid>,
    user_id: Uuid,
) -> Result<HashSet<Uuid>, AppError> {
    if candidate_ids.is_empty() {
        return Ok(HashSet::new());
    }

    // メンバーを 1 人以上指定しているプロジェクト（= テナント全体には開放されない）
    let restricted: HashSet<Uuid> = project_members::Entity::find()
        .filter(project_members::Column::ProjectId.is_in(candidate_ids.clone()))
        .select_only()
        .column(project_members::Column::ProjectId)
        .distinct()
        .into_tuple::<Uuid>()
        .all(db)
        .await?
        .into_iter()
        .collect();

    // そのうち自分が指定されているもの
    let mine: HashSet<Uuid> = project_members::Entity::find()
        .filter(project_members::Column::ProjectId.is_in(candidate_ids.clone()))
        .filter(project_members::Column::UserId.eq(user_id))
        .select_only()
        .column(project_members::Column::ProjectId)
        .into_tuple::<Uuid>()
        .all(db)
        .await?
        .into_iter()
        .collect();

    Ok(candidate_ids
        .into_iter()
        .filter(|id| !restricted.contains(id) || mine.contains(id))
        .collect())
}

/// そのプロジェクトに入れる利用者（テナントオーナーは含まない）。
///
/// 通知やメンションの宛先を絞るために使う。`project_is_open_or_member` と同じルールで、
/// メンバー未指定のプロジェクトはテナントメンバー全員が宛先になる。
pub async fn project_accessible_user_ids<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
) -> Result<HashSet<Uuid>, AppError> {
    let Some(project) = projects::Entity::find_by_id(project_id).one(db).await? else {
        return Ok(HashSet::new());
    };

    let members: HashSet<Uuid> = project_members::Entity::find()
        .filter(project_members::Column::ProjectId.eq(project_id))
        .select_only()
        .column(project_members::Column::UserId)
        .into_tuple::<Uuid>()
        .all(db)
        .await?
        .into_iter()
        .collect();
    if !members.is_empty() {
        return Ok(members);
    }

    Ok(tenant_members::Entity::find()
        .filter(tenant_members::Column::TenantId.eq(project.tenant_id))
        .select_only()
        .column(tenant_members::Column::UserId)
        .into_tuple::<Uuid>()
        .all(db)
        .await?
        .into_iter()
        .collect())
}
