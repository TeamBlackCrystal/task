//! テナント / プロジェクトのアクセス判定（#568）。
//!
//! 認可（handler）と通知の宛先抽出（service）が同じルールを見る必要があるため、
//! ここに一本化する。handler 側は `auth_helpers` が再公開している。

use std::collections::HashSet;

use sea_orm::{
    ColumnTrait, Condition, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter, QuerySelect,
    prelude::Uuid,
};

use common::error::AppError;
use entity::{project_members, projects, tenant_members, tenants};

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
/// 個人プロジェクト（Inbox）は、メンバー行が消えても開放しない。
/// 作成時には本人が `project_members` に入る（`my_tasks::seed_personal_project_defaults`）が、
/// テナントメンバーの削除や利用者の削除でその行は消えうるため、行の有無に頼らず
/// `is_personal` で明示的に閉じる。
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

    if project_members::Entity::find()
        .filter(project_members::Column::ProjectId.eq(project_id))
        .count(db)
        .await?
        != 0
    {
        return Ok(false);
    }

    // メンバー 0 人でテナント全体に開放するのは共有プロジェクトだけ。
    // 個人プロジェクト（Inbox）はメンバー行が消えても本人以外に開けない
    let Some(project) = projects::Entity::find_by_id(project_id).one(db).await? else {
        return Ok(false);
    };
    Ok(!project.is_personal || project.personal_owner_id == Some(user_id))
}

/// そのプロジェクトのタスクに担当者として指定できる利用者。
///
/// 判定規則は handler の `require_project_access` と同じ:
/// テナントオーナーは常に可、テナントメンバーは `project_is_open_or_member` を満たせば可。
/// 1 人ずつ問い合わせると人数分のクエリになるため、候補一覧はここで集合として返す。
///
/// **テナントに入れることは呼び出し側で確認済みの前提。**
pub async fn assignable_user_ids<C: ConnectionTrait>(
    db: &C,
    tenant_id: Uuid,
    project_id: Uuid,
) -> Result<Vec<Uuid>, AppError> {
    let Some(tenant) = tenants::Entity::find_by_id(tenant_id).one(db).await? else {
        return Ok(Vec::new());
    };
    let Some(project) = projects::Entity::find_by_id(project_id).one(db).await? else {
        return Ok(Vec::new());
    };

    // このプロジェクトに明示指定されている利用者。空ならテナント全体へ開放されている
    // （ただし個人プロジェクトは開放しない。project_is_open_or_member と同じ扱い）
    let named: HashSet<Uuid> = project_members::Entity::find()
        .filter(project_members::Column::ProjectId.eq(project_id))
        .select_only()
        .column(project_members::Column::UserId)
        .into_tuple::<Uuid>()
        .all(db)
        .await?
        .into_iter()
        .collect();

    let members: Vec<Uuid> = tenant_members::Entity::find()
        .filter(tenant_members::Column::TenantId.eq(tenant_id))
        .select_only()
        .column(tenant_members::Column::UserId)
        .into_tuple::<Uuid>()
        .all(db)
        .await?;

    let mut ids = Vec::with_capacity(members.len() + 1);
    let mut seen = HashSet::new();
    // オーナーはプロジェクト指定に関係なく可（require_project_access と同じ）
    if seen.insert(tenant.owner_id) {
        ids.push(tenant.owner_id);
    }
    for user_id in members {
        let allowed = if named.contains(&user_id) {
            true
        } else if !named.is_empty() {
            false
        } else {
            !project.is_personal || project.personal_owner_id == Some(user_id)
        };
        if allowed && seen.insert(user_id) {
            ids.push(user_id);
        }
    }
    Ok(ids)
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

    // 他人の個人プロジェクト（Inbox）は、メンバー行が消えても開放しない
    let foreign_personal: HashSet<Uuid> = projects::Entity::find()
        .filter(projects::Column::Id.is_in(candidate_ids.clone()))
        .filter(projects::Column::IsPersonal.eq(true))
        // NULL != user_id は NULL 判定になり素通りするので、明示的に落とす
        .filter(
            Condition::any()
                .add(projects::Column::PersonalOwnerId.ne(user_id))
                .add(projects::Column::PersonalOwnerId.is_null()),
        )
        .select_only()
        .column(projects::Column::Id)
        .into_tuple::<Uuid>()
        .all(db)
        .await?
        .into_iter()
        .collect();

    Ok(candidate_ids
        .into_iter()
        // 他人の個人プロジェクトでも、明示的に指定されていれば見える
        // （`project_is_open_or_member` が自分の行を先に見るのと同じ扱い）
        .filter(|id| !foreign_personal.contains(id) || mine.contains(id))
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

    let tenant_member_ids: HashSet<Uuid> = tenant_members::Entity::find()
        .filter(tenant_members::Column::TenantId.eq(project.tenant_id))
        .select_only()
        .column(tenant_members::Column::UserId)
        .into_tuple::<Uuid>()
        .all(db)
        .await?
        .into_iter()
        .collect();

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
        // テナントから外れた人の行は残る（`tenant_members::remove_member`）ので宛先から落とす。
        // そのプロジェクトに入れない人に通知を送らないため
        return Ok(members.intersection(&tenant_member_ids).copied().collect());
    }

    // メンバー 0 人で宛先をテナント全体に広げるのは共有プロジェクトだけ
    if project.is_personal {
        return Ok(project.personal_owner_id.into_iter().collect());
    }

    Ok(tenant_member_ids)
}
