mod common;

use axum::http::StatusCode;
use common::{TestApp, TestUser, insert_personal_token_for_test};
use entity::projects;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ConnectionTrait, DatabaseConnection};
use serde_json::Value;
use uuid::Uuid;

/// project にだけ参加しテナントに参加しない「客分」（project-only guest）の一級化。
///
/// 客分は既存の唯一の経路で作る: テナントメンバーに追加 → プロジェクトへ明示指定 →
/// テナントから除名（`project_members` の行は残る。`tenant_members::remove_member`）。
///
/// ここで固定する契約（セッション・PAT 共通）:
/// 1. 名指しされた自分のプロジェクトへは 200（修正前は 403 — 赤）
/// 2. 明示指定の無い他プロジェクトへは 403（「メンバー未指定＝開放」はテナントメンバー限り）
/// 3. テナント全体の口（テナント取得・プロジェクト一覧）は従来どおり 403
/// 4. テナント一覧に membership=Guest の印付きで出る（修正前は出ない — 赤）
/// 5. 無関係な利用者は従来どおり入れない（権限を広げていない対照）
async fn insert_second_project(db: &DatabaseConnection, tenant_id: Uuid) -> Uuid {
    let id = Uuid::new_v4();
    let suffix = &id.to_string()[..8];
    projects::ActiveModel {
        id: Set(id),
        name: Set("guest-other".into()),
        description: Set(String::new()),
        tenant_id: Set(tenant_id),
        icon_emoji: Set(None),
        icon_url: Set(None),
        // テナントごとに一意なキー。project key 制約 ^[A-Z][A-Z0-9]{1,9}$ を満たす
        key: Set(format!("Q{}", suffix.to_uppercase())),
        is_personal: Set(false),
        personal_owner_id: Set(None),
    }
    .insert(db)
    .await
    .expect("insert second project");
    id
}

/// scopes を指定して PAT を挿す（`insert_personal_token_for_test` は admin:tenant 固定のため、
/// プロジェクト読み取りも試す本テスト用に read:project を足した版）。
async fn insert_pat_with_project_read(
    db: &DatabaseConnection,
    user_id: Uuid,
    tenant_id: Uuid,
    secret: &str,
) -> String {
    use backend::utils::auth::generate_personal_token;
    use sea_orm::Statement;

    let (token, token_hash) = generate_personal_token(secret).expect("generate pat");
    let id = Uuid::new_v4();
    let last_four = token[token.len().saturating_sub(4)..].to_string();
    let stmt = Statement::from_sql_and_values(
        db.get_database_backend(),
        r#"INSERT INTO personal_tokens
            (id, name, token_hash, token_last_four, user_id, tenant_id, revoked, scopes)
            VALUES ($1, $2, $3, $4, $5, $6, false, '["admin:tenant","read:project"]'::json)"#,
        vec![
            id.into(),
            "guest-integration-test".into(),
            token_hash.into(),
            last_four.into(),
            user_id.into(),
            tenant_id.into(),
        ],
    );
    db.execute_raw(stmt).await.expect("insert guest pat");
    token
}

struct GuestSetup {
    owner: TestUser,
    guest: TestUser,
    stranger: TestUser,
    tenant_id: Uuid,
    own_project_id: Uuid,
    other_project_id: Uuid,
}

/// owner がテナントと 2 プロジェクトを持ち、guest が P1 の明示メンバーのまま
/// テナントから除名された状態（= project-only の客分）を API 経由で作る。
async fn setup_guest(app: &mut TestApp) -> GuestSetup {
    let owner = app.insert_user(false, false).await;
    let guest = app.insert_user(false, false).await;
    let stranger = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;
    let other_project_id = insert_second_project(&app.state.db, tp.tenant_id).await;

    let members_path = format!("/v1/tenants/{}/members", tp.tenant_id);
    let project_members_path = format!(
        "/v1/tenants/{}/projects/{}/members",
        tp.tenant_id, tp.project_id
    );

    app.reset_session_client();
    app.login_session(&owner.email, &owner.password).await;
    let added = app
        .post_json_with_session(
            &members_path,
            serde_json::json!({ "user_id": guest.id, "role": "Member" }),
        )
        .await;
    assert_eq!(
        added.status(),
        StatusCode::CREATED,
        "guest をテナントへ追加"
    );
    let assigned = app
        .post_json_with_session(
            &project_members_path,
            serde_json::json!({ "user_id": guest.id, "role": "Member" }),
        )
        .await;
    assert_eq!(
        assigned.status(),
        StatusCode::CREATED,
        "guest をプロジェクトへ明示指定"
    );
    let removed = app
        .delete_with_session(&format!("{members_path}/{}", guest.id))
        .await;
    assert_eq!(
        removed.status(),
        StatusCode::NO_CONTENT,
        "guest をテナントから除名（project_members の行は残る）"
    );

    GuestSetup {
        owner,
        guest,
        stranger,
        tenant_id: tp.tenant_id,
        own_project_id: tp.project_id,
        other_project_id,
    }
}

/// テナント一覧レスポンスから対象テナントの membership 印を取り出す。
/// 一覧に出ていなければ None。
async fn membership_of(res: reqwest::Response, tenant_id: Uuid) -> Option<String> {
    assert_eq!(res.status(), StatusCode::OK, "テナント一覧は 200");
    let body: Value = res.json().await.expect("tenant list json");
    let wanted = tenant_id.to_string();
    body.as_array()
        .expect("tenant list must be an array")
        .iter()
        .find(|t| t["id"].as_str() == Some(wanted.as_str()))
        .map(|t| {
            t["membership"]
                .as_str()
                .expect("membership must be a string")
                .to_string()
        })
}

/// セッションの客分: 名指しの中だけ通り、テナント全体の口は閉じたまま、一覧に印が出る。
#[tokio::test]
async fn session_guest_passes_only_named_project_and_is_marked_in_list() {
    let mut app = TestApp::new().await;
    let s = setup_guest(&mut app).await;

    let tenant_path = format!("/v1/tenants/{}", s.tenant_id);
    let projects_path = format!("/v1/tenants/{}/projects", s.tenant_id);
    let own_path = format!("{projects_path}/{}", s.own_project_id);
    let other_path = format!("{projects_path}/{}", s.other_project_id);

    app.reset_session_client();
    app.login_session(&s.guest.email, &s.guest.password).await;

    // ① 名指しされた自分のプロジェクトは通る（修正前は 403 — 赤）
    assert_eq!(
        app.get_with_session(&own_path).await.status(),
        StatusCode::OK,
        "客分は明示指定されたプロジェクトへ入れる"
    );
    // ② 明示指定の無い他プロジェクトは通さない（メンバー未指定の開放はテナントメンバー限り）
    assert_eq!(
        app.get_with_session(&other_path).await.status(),
        StatusCode::FORBIDDEN,
        "メンバー未指定のプロジェクトは客分に開かない"
    );
    // ③ テナント全体の口は従来どおり 403
    assert_eq!(
        app.get_with_session(&tenant_path).await.status(),
        StatusCode::FORBIDDEN,
        "客分にテナント取得は開かない（tenant-wide は従来どおり）"
    );
    assert_eq!(
        app.get_with_session(&projects_path).await.status(),
        StatusCode::FORBIDDEN,
        "客分にプロジェクト一覧は開かない（tenant-wide は従来どおり）"
    );
    // ④ テナント一覧に Guest の印付きで出る（修正前は出ない — 赤）
    assert_eq!(
        membership_of(app.get_with_session("/v1/tenants").await, s.tenant_id).await,
        Some("Guest".to_string()),
        "客分のテナントは membership=Guest の印付きで一覧に出る"
    );

    // 対照: オーナーには Owner の印
    app.reset_session_client();
    app.login_session(&s.owner.email, &s.owner.password).await;
    assert_eq!(
        membership_of(app.get_with_session("/v1/tenants").await, s.tenant_id).await,
        Some("Owner".to_string()),
        "オーナーは membership=Owner の印付きで一覧に出る"
    );

    // ⑤ 無関係な利用者は従来どおり入れない（権限を広げていない対照）
    app.reset_session_client();
    app.login_session(&s.stranger.email, &s.stranger.password)
        .await;
    assert_eq!(
        app.get_with_session(&own_path).await.status(),
        StatusCode::FORBIDDEN,
        "無関係な利用者はプロジェクトへ入れない"
    );
    assert_eq!(
        membership_of(app.get_with_session("/v1/tenants").await, s.tenant_id).await,
        None,
        "無関係な利用者の一覧にこのテナントは出ない"
    );

    app.cleanup_user(s.guest.id).await;
    app.cleanup_user(s.stranger.id).await;
    app.cleanup_user(s.owner.id).await;
}

/// PAT の客分: セッションと同じ扱い（バインド・scope の層は変えない）。
#[tokio::test]
async fn pat_guest_passes_only_named_project_and_is_marked_in_list() {
    let mut app = TestApp::new().await;
    let s = setup_guest(&mut app).await;

    let secret = app.state.settings.personal_token_secret.clone();
    let guest_pat =
        insert_pat_with_project_read(&app.state.db, s.guest.id, s.tenant_id, &secret).await;
    let owner_pat =
        insert_personal_token_for_test(&app.state.db, s.owner.id, s.tenant_id, &secret).await;

    let tenant_path = format!("/v1/tenants/{}", s.tenant_id);
    let projects_path = format!("/v1/tenants/{}/projects", s.tenant_id);
    let own_path = format!("{projects_path}/{}", s.own_project_id);
    let other_path = format!("{projects_path}/{}", s.other_project_id);

    // ① 名指しされた自分のプロジェクトは通る（修正前は 403 — 赤）
    assert_eq!(
        app.get_with_bearer(&own_path, &guest_pat).await.status(),
        StatusCode::OK,
        "客分の PAT は明示指定されたプロジェクトへ入れる"
    );
    // ② 明示指定の無い他プロジェクトは通さない
    assert_eq!(
        app.get_with_bearer(&other_path, &guest_pat).await.status(),
        StatusCode::FORBIDDEN,
        "メンバー未指定のプロジェクトは客分の PAT にも開かない"
    );
    // ③ テナント全体の口は従来どおり 403
    assert_eq!(
        app.get_with_bearer(&tenant_path, &guest_pat).await.status(),
        StatusCode::FORBIDDEN,
        "客分の PAT にテナント取得は開かない"
    );
    assert_eq!(
        app.get_with_bearer(&projects_path, &guest_pat)
            .await
            .status(),
        StatusCode::FORBIDDEN,
        "客分の PAT にプロジェクト一覧は開かない"
    );
    // ④ テナント一覧に Guest の印付きで出る（修正前は出ない — 赤）
    assert_eq!(
        membership_of(
            app.get_with_bearer("/v1/tenants", &guest_pat).await,
            s.tenant_id
        )
        .await,
        Some("Guest".to_string()),
        "客分の PAT でもテナント一覧に membership=Guest の印付きで出る"
    );

    // 対照: オーナーの PAT には Owner の印
    assert_eq!(
        membership_of(
            app.get_with_bearer("/v1/tenants", &owner_pat).await,
            s.tenant_id
        )
        .await,
        Some("Owner".to_string()),
        "オーナーの PAT は membership=Owner の印付きで出る"
    );

    app.cleanup_user(s.guest.id).await;
    app.cleanup_user(s.stranger.id).await;
    app.cleanup_user(s.owner.id).await;
}
