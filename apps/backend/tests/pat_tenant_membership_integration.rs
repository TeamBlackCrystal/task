mod common;

use axum::http::StatusCode;
use common::{TestApp, insert_personal_token_for_test, insert_tenant};
use entity::tenant_members;
use entity::tenants;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, DatabaseConnection, EntityTrait};
use uuid::Uuid;

/// PAT が「認証は通る（401 でない）のに読み口が 403」になるテナント所属の不備の
/// 再現と回帰。
///
/// `has_tenant_access`（handler/src/extractors.rs）は tenants.owner_id の一致（オーナー近道）
/// → tenant_members の行存在の順で所属を判定する。オーナーは tenant_members に行を持たない
/// 設計（apps/backend/docs/tenant-project-authz.md）のため、owner_id が本人以外を指した時点で
/// 行の無い利用者は両方の判定から外れ、バインド先テナントの全 API が 403 になる。
///
/// ここで固定する契約:
/// 1. その 403 の body が理由（tenant-membership-missing）を名指しする
///    （修正前は一般の "forbidden" しか返らず、原因へ辿り着けない — 修正前に赤）
/// 2. tenant_members に行を戻せばアクセスが回復する（欠落が原因である証明）
/// 3. 権限は広がっていない: 所属が無い間は 403 のまま（status は変えない）
async fn insert_tenant_member(db: &DatabaseConnection, tenant_id: Uuid, user_id: Uuid) {
    tenant_members::ActiveModel {
        id: Set(Uuid::new_v4()),
        tenant_id: Set(tenant_id),
        user_id: Set(user_id),
        role: Set(tenant_members::TenantRole::Member),
    }
    .insert(db)
    .await
    .expect("insert tenant member");
}

async fn set_tenant_owner(db: &DatabaseConnection, tenant_id: Uuid, owner_id: Uuid) {
    let tenant = tenants::Entity::find_by_id(tenant_id)
        .one(db)
        .await
        .expect("find tenant")
        .expect("tenant exists");
    let mut active: tenants::ActiveModel = tenant.into();
    active.owner_id = Set(owner_id);
    active.update(db).await.expect("update tenant owner");
}

/// owner_id が別人へ付け替わり tenant_members に行が無い利用者の PAT は、
/// 認証は通るが所属欠落の 403 が理由付きで返り、行を戻せば回復する。
#[tokio::test]
async fn pat_reports_membership_missing_when_owner_drifts() {
    let app = TestApp::new().await;

    let user = app.insert_user_default().await;
    let other = app.insert_user_default().await;
    let tenant_id = insert_tenant(&app.state.db, user.id).await;

    let secret = app.state.settings.personal_token_secret.clone();
    let token = insert_personal_token_for_test(&app.state.db, user.id, tenant_id, &secret).await;
    let path = format!("/v1/tenants/{tenant_id}");

    // 陽性対照: オーナーのうちは PAT で読める（owner 近道。ここが通らねば以降は何も証明しない）
    let res = app.get_with_bearer(&path, &token).await;
    assert_eq!(res.status(), StatusCode::OK, "オーナーのうちは読める");

    // 症状を再現: owner_id を別人へ付け替える。tenant_members に本人の行は無いまま
    // （オーナーは行を持たない設計ゆえ、これで所属の両判定から外れる）
    set_tenant_owner(&app.state.db, tenant_id, other.id).await;

    // 認証は通る（401 でない）が、所属欠落の 403 が理由を名指しして返る
    let res = app.get_with_bearer(&path, &token).await;
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "所属が無ければ 403（401 ではない = 認証は通っている）"
    );
    let body = res.json::<serde_json::Value>().await.expect("error body");
    assert_eq!(
        body["message"], "tenant-membership-missing",
        "一般の forbidden と見分けが付く理由を返す"
    );

    // 欠落が原因である証明: tenant_members に行を戻すとアクセスが回復する
    insert_tenant_member(&app.state.db, tenant_id, user.id).await;
    let res = app.get_with_bearer(&path, &token).await;
    assert_eq!(res.status(), StatusCode::OK, "メンバー行があれば回復する");

    app.cleanup_user(user.id).await;
    app.cleanup_user(other.id).await;
}

/// 対照: 存在しないトークンは 403 ではなく 401（「認証は通るが 403」との切り分け）。
#[tokio::test]
async fn invalid_pat_is_unauthorized_not_forbidden() {
    let app = TestApp::new().await;

    let user = app.insert_user_default().await;
    let tenant_id = insert_tenant(&app.state.db, user.id).await;

    let res = app
        .get_with_bearer(&format!("/v1/tenants/{tenant_id}"), "kt_not_a_real_token")
        .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    app.cleanup_user(user.id).await;
}
