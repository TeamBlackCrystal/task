mod common;

use axum::http::StatusCode;
use common::{TestApp, insert_personal_token_for_test};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::Value;
use uuid::Uuid;

/// テナントメンバー（#568）の統合テスト。
///
/// - テナントに入れるのはオーナーとテナントメンバーだけ
/// - プロジェクトにメンバーを 1 人も指定していなければ、テナントメンバー全員が入れる
/// - プロジェクトにメンバーを指定した場合は、その中に居る人だけが入れる
#[tokio::test]
async fn tenant_members_gate_tenant_and_project_access() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let member = app.insert_user(false, false).await;
    let outsider = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;

    let members_path = format!("/v1/tenants/{}/members", tp.tenant_id);
    let tenant_path = format!("/v1/tenants/{}", tp.tenant_id);
    let project_path = format!("/v1/tenants/{}/projects/{}", tp.tenant_id, tp.project_id);

    // --- 追加前: メンバーでない人はテナントを見られない ---
    app.reset_session_client();
    app.login_session(&member.email, &member.password).await;
    assert_eq!(
        app.get_with_session(&tenant_path).await.status(),
        StatusCode::FORBIDDEN,
        "テナントメンバーでなければテナントを取得できない"
    );
    assert!(
        !tenant_ids(app.get_with_session("/v1/tenants").await)
            .await
            .contains(&tp.tenant_id),
        "テナントメンバーでなければ一覧にも出ない"
    );

    // --- オーナーがテナントメンバーに追加する ---
    app.reset_session_client();
    app.login_session(&owner.email, &owner.password).await;
    let added = app
        .post_json_with_session(
            &members_path,
            serde_json::json!({ "user_id": member.id, "role": "Member" }),
        )
        .await;
    assert_eq!(added.status(), StatusCode::CREATED);

    // 同じ人を二重に追加すると 409
    let duplicated = app
        .post_json_with_session(
            &members_path,
            serde_json::json!({ "user_id": member.id, "role": "Member" }),
        )
        .await;
    assert_eq!(duplicated.status(), StatusCode::CONFLICT);

    // owner は tenant_members に行を持たないが、管理画面の一覧には表示する。
    let listed = app.get_with_session(&members_path).await;
    assert_eq!(listed.status(), StatusCode::OK);
    let listed_body: Value = listed.json().await.expect("tenant members json");
    let listed_ids: Vec<Uuid> = listed_body
        .as_array()
        .expect("tenant members must be an array")
        .iter()
        .map(|member| {
            member["user_id"]
                .as_str()
                .and_then(|id| Uuid::parse_str(id).ok())
                .expect("member user_id")
        })
        .collect();
    assert!(listed_ids.contains(&owner.id));
    assert!(listed_ids.contains(&member.id));

    // --- 追加後: テナントもプロジェクトも見える（project_members が空なので開放） ---
    app.reset_session_client();
    app.login_session(&member.email, &member.password).await;
    assert_eq!(
        app.get_with_session(&tenant_path).await.status(),
        StatusCode::OK,
        "テナントメンバーはテナントを取得できる"
    );
    assert!(
        tenant_ids(app.get_with_session("/v1/tenants").await)
            .await
            .contains(&tp.tenant_id),
        "テナントメンバーは一覧に出る"
    );
    assert_eq!(
        app.get_with_session(&project_path).await.status(),
        StatusCode::OK,
        "メンバー未指定のプロジェクトはテナントメンバー全員に開放される"
    );

    // 無関係なユーザーは依然として入れない（過剰に開放していないこと）
    app.reset_session_client();
    app.login_session(&outsider.email, &outsider.password).await;
    assert_eq!(
        app.get_with_session(&tenant_path).await.status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.get_with_session(&project_path).await.status(),
        StatusCode::FORBIDDEN
    );

    // --- プロジェクトにメンバーを指定すると、その人以外は弾かれる ---
    app.reset_session_client();
    app.login_session(&owner.email, &owner.password).await;
    let project_members_path = format!(
        "/v1/tenants/{}/projects/{}/members",
        tp.tenant_id, tp.project_id
    );
    // テナントに居ない人はプロジェクトメンバーにできない
    // （「プロジェクトには居るがテナントには入れない」不整合を作らせない）
    let rejected = app
        .post_json_with_session(
            &project_members_path,
            serde_json::json!({ "user_id": outsider.id, "role": "Member" }),
        )
        .await;
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);

    // テナントメンバーを 1 人プロジェクトに指定する
    let assignee = app.insert_user(false, false).await;
    let joined = app
        .post_json_with_session(
            &members_path,
            serde_json::json!({ "user_id": assignee.id, "role": "Member" }),
        )
        .await;
    assert_eq!(joined.status(), StatusCode::CREATED);
    let assigned = app
        .post_json_with_session(
            &project_members_path,
            serde_json::json!({ "user_id": assignee.id, "role": "Member" }),
        )
        .await;
    assert_eq!(assigned.status(), StatusCode::CREATED);

    app.reset_session_client();
    app.login_session(&member.email, &member.password).await;
    assert_eq!(
        app.get_with_session(&tenant_path).await.status(),
        StatusCode::OK,
        "テナント自体には引き続き入れる"
    );
    assert_eq!(
        app.get_with_session(&project_path).await.status(),
        StatusCode::FORBIDDEN,
        "メンバーを指定したプロジェクトは、指定された人以外は入れない"
    );

    app.cleanup_user(assignee.id).await;
    app.cleanup_user(member.id).await;
    app.cleanup_user(outsider.id).await;
    app.cleanup_user(owner.id).await;
}

/// メンバーの追加・変更・削除はオーナーとテナント Admin だけに許す。
#[tokio::test]
async fn only_owner_and_tenant_admin_can_manage_members() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let admin = app.insert_user(false, false).await;
    let plain = app.insert_user(false, false).await;
    let target = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;
    let members_path = format!("/v1/tenants/{}/members", tp.tenant_id);

    app.reset_session_client();
    app.login_session(&owner.email, &owner.password).await;
    for (user, role) in [(&admin, "Admin"), (&plain, "Member")] {
        let res = app
            .post_json_with_session(
                &members_path,
                serde_json::json!({ "user_id": user.id, "role": role }),
            )
            .await;
        assert_eq!(res.status(), StatusCode::CREATED);
    }

    // Member ロールでは追加できない
    app.reset_session_client();
    app.login_session(&plain.email, &plain.password).await;
    let rejected = app
        .post_json_with_session(
            &members_path,
            serde_json::json!({ "user_id": target.id, "role": "Member" }),
        )
        .await;
    assert_eq!(rejected.status(), StatusCode::FORBIDDEN);
    // 一覧の閲覧は許す（管理操作ではない）
    assert_eq!(
        app.get_with_session(&members_path).await.status(),
        StatusCode::OK
    );

    // Admin ロールなら追加・変更・削除できる
    app.reset_session_client();
    app.login_session(&admin.email, &admin.password).await;
    let accepted = app
        .post_json_with_session(
            &members_path,
            serde_json::json!({ "user_id": target.id, "role": "Viewer" }),
        )
        .await;
    assert_eq!(accepted.status(), StatusCode::CREATED);

    let target_path = format!("{members_path}/{}", target.id);
    let updated = app
        .put_json_with_session(&target_path, serde_json::json!({ "role": "Member" }))
        .await;
    assert_eq!(updated.status(), StatusCode::OK);
    let updated_body: Value = updated.json().await.expect("updated json");
    assert_eq!(updated_body["role"], "Member");

    assert_eq!(
        app.delete_with_session(&target_path).await.status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.delete_with_session(&target_path).await.status(),
        StatusCode::NOT_FOUND,
        "削除済みのメンバーは 404"
    );

    app.cleanup_user(admin.id).await;
    app.cleanup_user(plain.id).await;
    app.cleanup_user(target.id).await;
    app.cleanup_user(owner.id).await;
}

/// 個人プロジェクト（Inbox）が「メンバー未指定＝テナント全体に開放」に
/// 巻き込まれないことのガード。
///
/// 今は `seed_personal_project_defaults` が作成時に本人を project_members へ入れるので
/// 開放対象にならないが、そこが変わると他人の Inbox が全テナントメンバーに開く。
#[tokio::test]
async fn personal_project_is_not_open_to_other_tenant_members() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let alice = app.insert_user(false, false).await;
    let bob = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;
    let members_path = format!("/v1/tenants/{}/members", tp.tenant_id);

    app.reset_session_client();
    app.login_session(&owner.email, &owner.password).await;
    for user in [&alice, &bob] {
        let res = app
            .post_json_with_session(
                &members_path,
                serde_json::json!({ "user_id": user.id, "role": "Member" }),
            )
            .await;
        assert_eq!(res.status(), StatusCode::CREATED);
    }

    // bob の個人プロジェクトを作らせる
    app.reset_session_client();
    app.login_session(&bob.email, &bob.password).await;
    let personal = app
        .get_with_session(&format!(
            "/v1/tenants/{}/users/me/personal-project",
            tp.tenant_id
        ))
        .await;
    assert_eq!(personal.status(), StatusCode::OK);
    let body: Value = personal.json().await.expect("personal project json");
    assert_eq!(body["is_personal"], true);
    let personal_id = body["id"].as_str().expect("personal project id");
    let personal_uuid: Uuid = personal_id.parse().expect("personal project uuid");
    let personal_tasks_path = format!(
        "/v1/tenants/{}/projects/{}/tasks",
        tp.tenant_id, personal_id
    );

    // 本人は入れる（過剰に拒否していないこと）
    assert_eq!(
        app.get_with_session(&personal_tasks_path).await.status(),
        StatusCode::OK,
        "個人プロジェクトには本人が入れる"
    );

    // 同じテナントの別メンバーは入れない
    app.reset_session_client();
    app.login_session(&alice.email, &alice.password).await;
    assert_eq!(
        app.get_with_session(&personal_tasks_path).await.status(),
        StatusCode::FORBIDDEN,
        "他人の個人プロジェクトはテナントメンバーでも入れない"
    );

    // メンバー行が消えても開放しない。
    // 利用者削除（`admin_users`）などで Inbox の project_members が 0 件になっても、
    // 「メンバー未指定＝テナント全体に開放」の規則に流れ込まないことを見る
    entity::project_members::Entity::delete_many()
        .filter(entity::project_members::Column::ProjectId.eq(personal_uuid))
        .exec(&app.state.db)
        .await
        .expect("delete personal project members");

    assert_eq!(
        app.get_with_session(&personal_tasks_path).await.status(),
        StatusCode::FORBIDDEN,
        "メンバー行が 0 件になっても他人の個人プロジェクトは開かない"
    );

    app.reset_session_client();
    app.login_session(&bob.email, &bob.password).await;
    assert_eq!(
        app.get_with_session(&personal_tasks_path).await.status(),
        StatusCode::OK,
        "メンバー行が消えても本人は自分の個人プロジェクトに入れる"
    );

    // bob をテナントから外しても開放されない
    app.reset_session_client();
    app.login_session(&owner.email, &owner.password).await;
    assert_eq!(
        app.delete_with_session(&format!("{members_path}/{}", bob.id))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );

    app.reset_session_client();
    app.login_session(&alice.email, &alice.password).await;
    assert_eq!(
        app.get_with_session(&personal_tasks_path).await.status(),
        StatusCode::FORBIDDEN,
        "テナントから外した人の個人プロジェクトも他のメンバーには開かない"
    );

    // 個人プロジェクトが作った drive_folders が bob を参照するので、
    // 先にテナントごと消す（オーナー削除でテナントが CASCADE される）
    app.cleanup_user(owner.id).await;
    app.cleanup_user(alice.id).await;
    app.cleanup_user(bob.id).await;
}

/// テナントから外しても、プロジェクトの絞り込みは壊さない。
/// project_members の行を消すと、その人しか指定されていなかったプロジェクトが
/// メンバー 0 人になり、「メンバー未指定＝テナント全体に開放」の規則で他のメンバーに開いてしまう。
#[tokio::test]
async fn removing_tenant_member_keeps_project_scoping() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let alice = app.insert_user(false, false).await;
    let bob = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;

    let members_path = format!("/v1/tenants/{}/members", tp.tenant_id);
    let project_path = format!("/v1/tenants/{}/projects/{}", tp.tenant_id, tp.project_id);
    let project_members_path = format!(
        "/v1/tenants/{}/projects/{}/members",
        tp.tenant_id, tp.project_id
    );

    app.reset_session_client();
    app.login_session(&owner.email, &owner.password).await;
    for user in [&alice, &bob] {
        let res = app
            .post_json_with_session(
                &members_path,
                serde_json::json!({ "user_id": user.id, "role": "Member" }),
            )
            .await;
        assert_eq!(res.status(), StatusCode::CREATED);
    }
    // alice だけをプロジェクトに指定する（= 絞り込み状態にする）
    let assigned = app
        .post_json_with_session(
            &project_members_path,
            serde_json::json!({ "user_id": alice.id, "role": "Member" }),
        )
        .await;
    assert_eq!(assigned.status(), StatusCode::CREATED);

    app.reset_session_client();
    app.login_session(&bob.email, &bob.password).await;
    assert_eq!(
        app.get_with_session(&project_path).await.status(),
        StatusCode::FORBIDDEN,
        "絞り込み状態のプロジェクトには指定された人しか入れない"
    );

    // alice をテナントから外す
    app.reset_session_client();
    app.login_session(&owner.email, &owner.password).await;
    assert_eq!(
        app.delete_with_session(&format!("{members_path}/{}", alice.id))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    let remaining = app.get_with_session(&project_members_path).await;
    assert_eq!(remaining.status(), StatusCode::OK);
    let remaining_body: Value = remaining.json().await.expect("project members json");
    assert_eq!(
        remaining_body
            .as_array()
            .expect("project members must be an array")
            .len(),
        1,
        "再参加したときに戻せるよう、プロジェクトの指定自体は残す"
    );

    // 絞り込みは壊れない。bob は依然として入れない
    app.reset_session_client();
    app.login_session(&bob.email, &bob.password).await;
    assert_eq!(
        app.get_with_session(&project_path).await.status(),
        StatusCode::FORBIDDEN,
        "メンバーを外しても、絞り込み済みのプロジェクトは他のメンバーに開かない"
    );

    // テナントから外れた alice も入れない（残った行はアクセスを与えない）
    app.reset_session_client();
    app.login_session(&alice.email, &alice.password).await;
    assert_eq!(
        app.get_with_session(&project_path).await.status(),
        StatusCode::FORBIDDEN,
        "テナントから外れた人は入れない"
    );

    // テナントに戻せば、元のプロジェクト指定がそのまま効く
    app.reset_session_client();
    app.login_session(&owner.email, &owner.password).await;
    assert_eq!(
        app.post_json_with_session(
            &members_path,
            serde_json::json!({ "user_id": alice.id, "role": "Member" }),
        )
        .await
        .status(),
        StatusCode::CREATED
    );

    app.reset_session_client();
    app.login_session(&alice.email, &alice.password).await;
    assert_eq!(
        app.get_with_session(&project_path).await.status(),
        StatusCode::OK,
        "テナントに戻したら元のプロジェクト指定が戻る"
    );

    app.cleanup_user(alice.id).await;
    app.cleanup_user(bob.id).await;
    app.cleanup_user(owner.id).await;
}

/// PAT のテナントバインドは「どのテナントを触れるか」の制限であって所属の証明ではない。
/// テナントから外した利用者のトークンで、テナント情報とメンバー一覧が読めてはいけない。
#[tokio::test]
async fn removed_member_pat_loses_tenant_read_access() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let alice = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;
    let members_path = format!("/v1/tenants/{}/members", tp.tenant_id);
    let tenant_path = format!("/v1/tenants/{}", tp.tenant_id);

    app.reset_session_client();
    app.login_session(&owner.email, &owner.password).await;
    let added = app
        .post_json_with_session(
            &members_path,
            serde_json::json!({ "user_id": alice.id, "role": "Member" }),
        )
        .await;
    assert_eq!(added.status(), StatusCode::CREATED);

    // alice のトークンはテナント T にバインドされ、プロジェクト制限は無い
    let alice_pat = insert_personal_token_for_test(
        &app.state.db,
        alice.id,
        tp.tenant_id,
        &app.state.settings.personal_token_secret,
    )
    .await;
    let owner_pat = insert_personal_token_for_test(
        &app.state.db,
        owner.id,
        tp.tenant_id,
        &app.state.settings.personal_token_secret,
    )
    .await;

    // メンバーで居るあいだは読める（過剰に拒否していないこと）
    assert_eq!(
        app.get_with_bearer(&tenant_path, &alice_pat).await.status(),
        StatusCode::OK,
        "テナントメンバーの PAT はテナントを取得できる"
    );
    assert_eq!(
        app.get_with_bearer(&members_path, &alice_pat)
            .await
            .status(),
        StatusCode::OK,
        "テナントメンバーの PAT はメンバー一覧を取得できる"
    );
    assert!(
        tenant_ids(app.get_with_bearer("/v1/tenants", &alice_pat).await)
            .await
            .contains(&tp.tenant_id),
        "テナントメンバーの PAT は一覧にそのテナントが出る"
    );

    // alice をテナントから外す
    app.reset_session_client();
    app.login_session(&owner.email, &owner.password).await;
    assert_eq!(
        app.delete_with_session(&format!("{members_path}/{}", alice.id))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );

    assert_eq!(
        app.get_with_bearer(&tenant_path, &alice_pat).await.status(),
        StatusCode::FORBIDDEN,
        "テナントから外した利用者の PAT でテナントを取得できてはいけない"
    );
    assert_eq!(
        app.get_with_bearer(&members_path, &alice_pat)
            .await
            .status(),
        StatusCode::FORBIDDEN,
        "テナントから外した利用者の PAT でメンバー一覧を取得できてはいけない"
    );
    // 取得が 403 でも一覧に名前や設定が出ては同じことなので、こちらも落ちること
    assert!(
        !tenant_ids(app.get_with_bearer("/v1/tenants", &alice_pat).await)
            .await
            .contains(&tp.tenant_id),
        "テナントから外した利用者の PAT ではテナント一覧にも出してはいけない"
    );

    // オーナーの PAT は影響を受けない
    assert_eq!(
        app.get_with_bearer(&tenant_path, &owner_pat).await.status(),
        StatusCode::OK,
        "オーナーの PAT は従来どおり通る"
    );
    assert!(
        tenant_ids(app.get_with_bearer("/v1/tenants", &owner_pat).await)
            .await
            .contains(&tp.tenant_id),
        "オーナーの PAT は一覧に従来どおり出る"
    );

    app.cleanup_user(alice.id).await;
    app.cleanup_user(owner.id).await;
}

async fn tenant_ids(res: reqwest::Response) -> Vec<Uuid> {
    let body: Value = res.json().await.expect("tenant list json");
    body.as_array()
        .expect("tenant list must be an array")
        .iter()
        .map(|t| {
            t["id"]
                .as_str()
                .and_then(|s| Uuid::parse_str(s).ok())
                .expect("tenant id")
        })
        .collect()
}

/// テナントから外した人の `project_members` の行は残る。
/// その行が Admin だと「最後の Admin は消せない」ガードに引っかかり、
/// もう管理操作を実行できない人がプロジェクトの Admin 枠を占有したまま外せなくなる。
#[tokio::test]
async fn removed_tenant_member_does_not_hold_last_project_admin_slot() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let alice = app.insert_user(false, false).await;
    let bob = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;

    let members_path = format!("/v1/tenants/{}/members", tp.tenant_id);
    let project_path = format!("/v1/tenants/{}/projects/{}", tp.tenant_id, tp.project_id);
    let project_members_path = format!(
        "/v1/tenants/{}/projects/{}/members",
        tp.tenant_id, tp.project_id
    );

    app.reset_session_client();
    app.login_session(&owner.email, &owner.password).await;
    for user in [&alice, &bob] {
        assert_eq!(
            app.post_json_with_session(
                &members_path,
                serde_json::json!({ "user_id": user.id, "role": "Member" }),
            )
            .await
            .status(),
            StatusCode::CREATED
        );
    }

    // alice をプロジェクト唯一の Admin にする
    assert_eq!(
        app.post_json_with_session(
            &project_members_path,
            serde_json::json!({ "user_id": alice.id, "role": "Admin" }),
        )
        .await
        .status(),
        StatusCode::CREATED
    );

    // テナントに居るあいだは最後の Admin として守られる（過剰に許可していないこと）
    assert_eq!(
        app.delete_with_session(&format!("{project_members_path}/{}", alice.id))
            .await
            .status(),
        StatusCode::CONFLICT,
        "テナントに居る最後の Admin は消せない"
    );
    assert_eq!(
        app.put_json_with_session(
            &format!("{project_members_path}/{}", alice.id),
            serde_json::json!({ "role": "Member" }),
        )
        .await
        .status(),
        StatusCode::CONFLICT,
        "テナントに居る最後の Admin は降格できない"
    );

    // alice をテナントから外す。project_members の行は残る
    assert_eq!(
        app.delete_with_session(&format!("{members_path}/{}", alice.id))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );

    // 残った行が Admin 枠を占有し続けないこと
    assert_eq!(
        app.delete_with_session(&format!("{project_members_path}/{}", alice.id))
            .await
            .status(),
        StatusCode::NO_CONTENT,
        "テナントに居ない Admin は最後の Admin として数えない"
    );

    // 後始末が済んだので、メンバー未指定に戻ったプロジェクトはテナントメンバーに開放される
    app.reset_session_client();
    app.login_session(&bob.email, &bob.password).await;
    assert_eq!(
        app.get_with_session(&project_path).await.status(),
        StatusCode::OK,
        "指定が無くなったプロジェクトはテナントメンバーに開放される"
    );

    app.cleanup_user(alice.id).await;
    app.cleanup_user(bob.id).await;
    app.cleanup_user(owner.id).await;
}

/// 管理者による利用者の強制削除でも、プロジェクトの絞り込みが壊れてはいけない。
///
/// `delete_user_cascade` が `project_members` の行を消していた頃は、
/// その人しか指定されていなかったプロジェクトがメンバー 0 件になり、
/// 「メンバー未指定はテナント全体に開放」の規則でテナント全員に開いてしまっていた（#568）。
#[tokio::test]
async fn deleting_user_keeps_project_scoping() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let admin = app.insert_user(true, false).await;
    let alice = app.insert_user(false, false).await;
    let bob = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;

    let members_path = format!("/v1/tenants/{}/members", tp.tenant_id);
    let project_path = format!("/v1/tenants/{}/projects/{}", tp.tenant_id, tp.project_id);
    let project_members_path = format!(
        "/v1/tenants/{}/projects/{}/members",
        tp.tenant_id, tp.project_id
    );

    app.reset_session_client();
    app.login_session(&owner.email, &owner.password).await;
    for user in [&alice, &bob] {
        let res = app
            .post_json_with_session(
                &members_path,
                serde_json::json!({ "user_id": user.id, "role": "Member" }),
            )
            .await;
        assert_eq!(res.status(), StatusCode::CREATED);
    }
    // alice だけをプロジェクトに指定する（= 絞り込み状態にする）
    let assigned = app
        .post_json_with_session(
            &project_members_path,
            serde_json::json!({ "user_id": alice.id, "role": "Member" }),
        )
        .await;
    assert_eq!(assigned.status(), StatusCode::CREATED);

    app.reset_session_client();
    app.login_session(&bob.email, &bob.password).await;
    assert_eq!(
        app.get_with_session(&project_path).await.status(),
        StatusCode::FORBIDDEN,
        "絞り込み状態のプロジェクトには指定された人しか入れない"
    );

    // 管理者が alice を強制削除する
    app.reset_session_client();
    app.login_session(&admin.email, &admin.password).await;
    assert_eq!(
        app.delete_with_session(&format!("/v1/admin/users/{}", alice.id))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );

    // 絞り込みは壊れない。bob は依然として入れない
    app.reset_session_client();
    app.login_session(&bob.email, &bob.password).await;
    assert_eq!(
        app.get_with_session(&project_path).await.status(),
        StatusCode::FORBIDDEN,
        "利用者を削除しても、絞り込み済みのプロジェクトは他のメンバーに開かない"
    );

    // 削除された利用者はテナントメンバーから外れている
    app.reset_session_client();
    app.login_session(&owner.email, &owner.password).await;
    let members = app.get_with_session(&members_path).await;
    assert_eq!(members.status(), StatusCode::OK);
    let members_body: Value = members.json().await.expect("tenant members json");
    let member_ids: Vec<Uuid> = members_body
        .as_array()
        .expect("tenant members must be an array")
        .iter()
        .map(|m| {
            m["user_id"]
                .as_str()
                .and_then(|s| Uuid::parse_str(s).ok())
                .expect("member user_id")
        })
        .collect();
    assert!(
        !member_ids.contains(&alice.id),
        "削除した利用者はテナントメンバー一覧に残ってはいけない"
    );
    assert!(member_ids.contains(&bob.id), "他のメンバーは影響を受けない");

    app.cleanup_user(alice.id).await;
    app.cleanup_user(bob.id).await;
    app.cleanup_user(admin.id).await;
    app.cleanup_user(owner.id).await;
}
