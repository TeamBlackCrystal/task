use sea_orm::{
    ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter, QuerySelect,
    prelude::Uuid,
};

use crate::AppState;
use crate::error::AppError;
use entity::{project_members, tenant_members};

// 実装は service 側に一本化（レビュー指摘: 同一実装の重複解消）。
pub use service::drive::is_tenant_owner;

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
pub async fn project_is_open_or_member<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    user_id: Uuid,
) -> Result<bool, AppError> {
    let member_count = project_members::Entity::find()
        .filter(project_members::Column::ProjectId.eq(project_id))
        .count(db)
        .await?;
    if member_count == 0 {
        return Ok(true);
    }
    Ok(project_members::Entity::find()
        .filter(project_members::Column::ProjectId.eq(project_id))
        .filter(project_members::Column::UserId.eq(user_id))
        .one(db)
        .await?
        .is_some())
}

/// 候補プロジェクトのうち、そのユーザーに見えるものを返す。
///
/// `project_is_open_or_member` をプロジェクトごとに呼ぶと件数分のクエリが出るため、
/// 一覧系ではこちらを使う（メンバー指定の有無と自分の所属を各 1 クエリで引く）。
/// **テナントに入れることは呼び出し側で確認済みの前提。**
pub async fn visible_project_ids<C: ConnectionTrait>(
    db: &C,
    candidate_ids: Vec<Uuid>,
    user_id: Uuid,
) -> Result<std::collections::HashSet<Uuid>, AppError> {
    use std::collections::HashSet;
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

pub async fn require_member_or_owner(
    state: &AppState,
    tenant_id: Uuid,
    project_id: Uuid,
    user_id: Uuid,
) -> Result<(), AppError> {
    if is_tenant_owner(&state.db, tenant_id, user_id).await? {
        return Ok(());
    }
    if !is_tenant_member(&state.db, tenant_id, user_id).await? {
        return Err(AppError::Forbidden);
    }
    if project_is_open_or_member(&state.db, project_id, user_id).await? {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}
