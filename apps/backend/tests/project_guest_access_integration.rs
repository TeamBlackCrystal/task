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
/// 3. テナント全体の口のうち、プロジェクト一覧（`GET …/projects`）と My Tasks（`GET …/users/me/tasks`）は
///    己が明示 member の project に絞って 200。その他の tenant-wide（テナント取得など）は 403
/// 4. テナント一覧に membership=Guest の印付きで出る（修正前は出ない — 赤）。
///    客分への応答ではテナント設定欄（owner_id / drive_quota_bytes / require_2fa）は null
/// 5. 無関係な利用者は従来どおり入れない（権限を広げていない対照）
/// 6. require_2fa テナントの 2FA 強制は客分にも及ぶ（修正前は漏れた — 赤）
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
            VALUES ($1, $2, $3, $4, $5, $6, false, '["admin:tenant","read:project","read:task"]'::json)"#,
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

/// プロジェクト一覧レスポンスから id を取り出す。
fn project_ids(body: Value) -> Vec<Uuid> {
    body.as_array()
        .expect("project list must be an array")
        .iter()
        .map(|p| {
            p["id"]
                .as_str()
                .and_then(|s| Uuid::parse_str(s).ok())
                .expect("project id")
        })
        .collect()
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
    // プロジェクト一覧だけは己が明示 member の project に絞って開く
    //（公開 project = other は含めぬ。修正前は 403 — 赤）
    let res = app.get_with_session(&projects_path).await;
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "客分にプロジェクト一覧は己の分に絞って開く"
    );
    assert_eq!(
        project_ids(res.json().await.expect("projects json")),
        vec![s.own_project_id],
        "客分の一覧は明示 member の project だけ（公開 project は含めぬ）"
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

/// 客分への一覧応答はテナント設定の欄（owner_id / drive_quota_bytes / require_2fa）を
/// 返さない（null）。Owner / Member には従来どおり値が入る。
#[tokio::test]
async fn guest_list_item_hides_tenant_settings() {
    let mut app = TestApp::new().await;
    let s = setup_guest(&mut app).await;

    // stranger をテナントメンバーへ追加して Member の対照にする
    app.reset_session_client();
    app.login_session(&s.owner.email, &s.owner.password).await;
    let added = app
        .post_json_with_session(
            &format!("/v1/tenants/{}/members", s.tenant_id),
            serde_json::json!({ "user_id": s.stranger.id, "role": "Member" }),
        )
        .await;
    assert_eq!(added.status(), StatusCode::CREATED, "対照の Member を用意");

    let find = |body: Value, tenant_id: Uuid| -> Value {
        let wanted = tenant_id.to_string();
        body.as_array()
            .expect("tenant list must be an array")
            .iter()
            .find(|t| t["id"].as_str() == Some(wanted.as_str()))
            .cloned()
            .expect("tenant must be listed")
    };

    // 客分: 三欄が null（欄は在るが値を返さない）
    app.reset_session_client();
    app.login_session(&s.guest.email, &s.guest.password).await;
    let res = app.get_with_session("/v1/tenants").await;
    assert_eq!(res.status(), StatusCode::OK);
    let item = find(res.json().await.expect("list json"), s.tenant_id);
    assert_eq!(item["membership"], "Guest");
    assert!(
        item["owner_id"].is_null(),
        "客分に owner_id を返さない: {item}"
    );
    assert!(
        item["drive_quota_bytes"].is_null(),
        "客分に drive_quota_bytes を返さない: {item}"
    );
    assert!(
        item["require_2fa"].is_null(),
        "客分に require_2fa を返さない: {item}"
    );
    // 一覧の表示に使う欄は残る
    assert!(item["display_id"].is_string());
    assert!(item["name"].is_string());
    assert!(item["description"].is_string());
    assert!(item["icon_url"].is_string());

    // Owner: 従来どおり値が入る
    app.reset_session_client();
    app.login_session(&s.owner.email, &s.owner.password).await;
    let item = find(
        app.get_with_session("/v1/tenants")
            .await
            .json()
            .await
            .expect("list json"),
        s.tenant_id,
    );
    assert_eq!(item["membership"], "Owner");
    assert_eq!(
        item["owner_id"].as_str(),
        Some(s.owner.id.to_string().as_str()),
        "Owner には owner_id が入る"
    );
    assert!(
        item["require_2fa"].is_boolean(),
        "Owner には require_2fa が入る"
    );

    // Member: 従来どおり値が入る
    app.reset_session_client();
    app.login_session(&s.stranger.email, &s.stranger.password)
        .await;
    let item = find(
        app.get_with_session("/v1/tenants")
            .await
            .json()
            .await
            .expect("list json"),
        s.tenant_id,
    );
    assert_eq!(item["membership"], "Member");
    assert_eq!(
        item["owner_id"].as_str(),
        Some(s.owner.id.to_string().as_str()),
        "Member には owner_id が入る"
    );
    assert!(
        item["require_2fa"].is_boolean(),
        "Member には require_2fa が入る"
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
    // プロジェクト一覧だけは PAT でも己の分に絞って開く（修正前は 403 — 赤）
    let res = app.get_with_bearer(&projects_path, &guest_pat).await;
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "客分の PAT にもプロジェクト一覧は己の分に絞って開く"
    );
    assert_eq!(
        project_ids(res.json().await.expect("projects json")),
        vec![s.own_project_id],
        "客分の PAT の一覧も明示 member の project だけ"
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

/// UI 経路の模擬: 客分が ①テナント一覧で己のテナントを得て ②プロジェクト一覧で
/// 己の project だけを得て ③その id で個別 API へ 200、着地の My Tasks も開ける。
/// tenant member の一覧は従来の規則のまま（公開 project は見え、
/// 絞り込み project は指定された人だけ）。セッションと PAT の両方。
#[tokio::test]
async fn ui_path_guest_reaches_own_project_and_member_list_stays_as_before() {
    let mut app = TestApp::new().await;
    let s = setup_guest(&mut app).await;

    // stranger をテナントメンバーへ追加して member の対照にする
    app.reset_session_client();
    app.login_session(&s.owner.email, &s.owner.password).await;
    let added = app
        .post_json_with_session(
            &format!("/v1/tenants/{}/members", s.tenant_id),
            serde_json::json!({ "user_id": s.stranger.id, "role": "Member" }),
        )
        .await;
    assert_eq!(added.status(), StatusCode::CREATED);

    let projects_path = format!("/v1/tenants/{}/projects", s.tenant_id);
    let my_tasks_path = format!("/v1/tenants/{}/users/me/tasks", s.tenant_id);

    // --- セッション
    app.reset_session_client();
    app.login_session(&s.guest.email, &s.guest.password).await;
    assert_eq!(
        membership_of(app.get_with_session("/v1/tenants").await, s.tenant_id).await,
        Some("Guest".to_string()),
        "① 客分はテナント一覧で己のテナントを得る"
    );
    let res = app.get_with_session(&projects_path).await;
    assert_eq!(res.status(), StatusCode::OK, "② プロジェクト一覧が開く");
    assert_eq!(
        project_ids(res.json().await.expect("projects json")),
        vec![s.own_project_id],
        "② 己の project だけが返る（他の project・公開 project は含めぬ）"
    );
    assert_eq!(
        app.get_with_session(&format!("{projects_path}/{}", s.own_project_id))
            .await
            .status(),
        StatusCode::OK,
        "③ 一覧で得た project の個別 API へ 200"
    );
    assert_eq!(
        app.get_with_session(&my_tasks_path).await.status(),
        StatusCode::OK,
        "テナント選択後の着地（My Tasks）が客分に開く"
    );

    // --- PAT でも同じ
    let secret = app.state.settings.personal_token_secret.clone();
    let guest_pat =
        insert_pat_with_project_read(&app.state.db, s.guest.id, s.tenant_id, &secret).await;
    let res = app.get_with_bearer(&projects_path, &guest_pat).await;
    assert_eq!(res.status(), StatusCode::OK, "② PAT でも一覧が開く");
    assert_eq!(
        project_ids(res.json().await.expect("projects json")),
        vec![s.own_project_id],
        "② PAT でも己の分だけ"
    );
    assert_eq!(
        app.get_with_bearer(&format!("{projects_path}/{}", s.own_project_id), &guest_pat)
            .await
            .status(),
        StatusCode::OK,
        "③ PAT でも個別 API へ 200"
    );
    assert_eq!(
        app.get_with_bearer(&my_tasks_path, &guest_pat)
            .await
            .status(),
        StatusCode::OK,
        "PAT でも My Tasks が開く"
    );

    // --- tenant member は従来の規則のまま
    app.reset_session_client();
    app.login_session(&s.stranger.email, &s.stranger.password)
        .await;
    let res = app.get_with_session(&projects_path).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        project_ids(res.json().await.expect("projects json")),
        vec![s.other_project_id],
        "member の一覧は従来規則のまま（公開 project は見え、絞り込み project は指定者だけ）"
    );

    app.cleanup_user(s.guest.id).await;
    app.cleanup_user(s.stranger.id).await;
    app.cleanup_user(s.owner.id).await;
}

/// require_2fa テナントの 2FA 強制は客分にも及ぶ（回避経路を閉じる）。
///
/// 修正前: 客分は tenant_members に行が無いため強制から漏れ、2FA 未設定でも
/// パスワードログインが本認証（204）になり、客分として project を読み書きできた — 赤。
#[tokio::test]
async fn guest_login_requires_2fa_setup_under_require_2fa() {
    let mut app = TestApp::new().await;
    let s = setup_guest(&mut app).await;

    // owner がテナントの 2FA 強制を立てる（owner の現セッションは本認証のまま）
    app.reset_session_client();
    app.login_session(&s.owner.email, &s.owner.password).await;
    let policy = app
        .post_json_with_session(
            &format!("/v1/tenants/{}/require-2fa", s.tenant_id),
            serde_json::json!({ "enabled": true }),
        )
        .await;
    assert_eq!(policy.status(), StatusCode::OK, "2FA 強制を有効化");

    // ① 2FA 未設定の客分のパスワードログインはセットアップ要求（half-authenticated）
    app.reset_session_client();
    let response = app
        .session_client()
        .post(format!("{}/v1/auth/login", app.base_url()))
        .json(&serde_json::json!({ "email": s.guest.email, "password": s.guest.password }))
        .send()
        .await
        .expect("guest login request");
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "① 客分のログインはセットアップ要求の JSON（204 の本認証ではない）"
    );
    let body: serde_json::Value = response.json().await.expect("login json");
    assert_eq!(
        body["requires_2fa_setup"].as_bool(),
        Some(true),
        "① require_2fa テナントの客分にはセットアップ要求が立つ"
    );

    // ② 2FA 設定完了前は、名指しの project の API も 403
    assert_eq!(
        app.get_with_session(&format!(
            "/v1/tenants/{}/projects/{}",
            s.tenant_id, s.own_project_id
        ))
        .await
        .status(),
        StatusCode::FORBIDDEN,
        "② half-authenticated のままでは客分の project も 403"
    );

    app.cleanup_user(s.guest.id).await;
    app.cleanup_user(s.stranger.id).await;
    app.cleanup_user(s.owner.id).await;
}

/// TOTP を有効にしている客分が require_2fa テナントに関わる限り、TOTP 無効化は拒否される。
///
/// 修正前: 客分のテナントは判定に入らず無効化が通り、次のログインから
/// 2FA 無しで客分アクセスできる状態へ移れた — 赤。
#[tokio::test]
async fn guest_cannot_disable_totp_under_require_2fa() {
    let mut app = TestApp::new().await;
    let s = setup_guest(&mut app).await;

    // guest2: テナントメンバーとして TOTP を有効化した後に除名し、TOTP 持ちの客分にする
    let guest2 = app.insert_user(false, false).await;
    let members_path = format!("/v1/tenants/{}/members", s.tenant_id);
    let project_members_path = format!(
        "/v1/tenants/{}/projects/{}/members",
        s.tenant_id, s.own_project_id
    );

    app.reset_session_client();
    app.login_session(&s.owner.email, &s.owner.password).await;
    let added = app
        .post_json_with_session(
            &members_path,
            serde_json::json!({ "user_id": guest2.id, "role": "Member" }),
        )
        .await;
    assert_eq!(added.status(), StatusCode::CREATED);
    let assigned = app
        .post_json_with_session(
            &project_members_path,
            serde_json::json!({ "user_id": guest2.id, "role": "Member" }),
        )
        .await;
    assert_eq!(assigned.status(), StatusCode::CREATED);

    // require_2fa が立つ前に TOTP を有効化（強制後は本認証ログインできないため）
    app.reset_session_client();
    let enabled = app.enable_2fa(&guest2).await;

    // 除名して客分にし、owner が 2FA 強制を立てる
    app.reset_session_client();
    app.login_session(&s.owner.email, &s.owner.password).await;
    let removed = app
        .delete_with_session(&format!("{members_path}/{}", guest2.id))
        .await;
    assert_eq!(removed.status(), StatusCode::NO_CONTENT);
    let policy = app
        .post_json_with_session(
            &format!("/v1/tenants/{}/require-2fa", s.tenant_id),
            serde_json::json!({ "enabled": true }),
        )
        .await;
    assert_eq!(policy.status(), StatusCode::OK);

    // guest2 がログイン（TOTP 持ちゆえ half）→ リカバリーコードで本認証
    app.reset_session_client();
    app.login_half_authed(&guest2).await;
    let verify = app
        .post_json_with_session(
            "/v1/auth/2fa/verify",
            serde_json::json!({ "recovery_code": &enabled.recovery_codes[0] }),
        )
        .await;
    assert_eq!(verify.status(), StatusCode::NO_CONTENT);

    // ③ require_2fa テナントに客分として関わる限り、TOTP 無効化は拒否される
    let delete = app
        .delete_json_with_session(
            "/v1/auth/2fa/totp",
            serde_json::json!({ "recovery_code": &enabled.recovery_codes[1] }),
        )
        .await;
    assert_eq!(
        delete.status(),
        StatusCode::FORBIDDEN,
        "③ 客分も require_2fa の対象ゆえ TOTP 無効化は 403"
    );

    app.cleanup_user(guest2.id).await;
    app.cleanup_user(s.guest.id).await;
    app.cleanup_user(s.stranger.id).await;
    app.cleanup_user(s.owner.id).await;
}
