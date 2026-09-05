mod common;

use axum::http::StatusCode;
use common::TestApp;
use entity::{drive_files, drive_folders, project_members, projects};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait, TransactionTrait};
use uuid::Uuid;

/// プロジェクトフォルダ配下の境界を守れているかを見る。
///
/// 直したのは 4 つ:
/// - 子フォルダを作っても `project_id` を継承せず、中のファイルが一般ファイル扱いになる
/// - フォルダ移動で移動元・移動先の ACL を見ず、配下の `project_id` も揃えない
/// - ファイル移動で移動先の ACL を見ない
/// - 自動生成のプロジェクトルートフォルダを直接削除・移動できる
async fn insert_extra_project(app: &TestApp, tenant_id: Uuid) -> Uuid {
    let project_id = Uuid::new_v4();
    let suffix = &project_id.to_string()[..8];
    projects::ActiveModel {
        id: Set(project_id),
        name: Set("second-project".into()),
        description: Set(String::new()),
        tenant_id: Set(tenant_id),
        icon_emoji: Set(None),
        icon_url: Set(None),
        key: Set(format!("Q{}", suffix.to_uppercase())),
        is_personal: Set(false),
        personal_owner_id: Set(None),
    }
    .insert(&app.state.db)
    .await
    .expect("insert project");
    project_id
}

/// プロジェクト作成時に自動生成されるのと同じ形（`project_id` 付き・親なし）のルートフォルダ。
async fn insert_project_root_folder(
    app: &TestApp,
    tenant_id: Uuid,
    project_id: Uuid,
    created_by: Uuid,
) -> Uuid {
    let folder_id = Uuid::new_v4();
    drive_folders::ActiveModel {
        id: Set(folder_id),
        name: Set("project-root".into()),
        parent_id: Set(None),
        tenant_id: Set(tenant_id),
        project_id: Set(Some(project_id)),
        created_by: Set(created_by),
        created_at: Set(chrono::Utc::now().into()),
    }
    .insert(&app.state.db)
    .await
    .expect("insert project root folder");
    folder_id
}

async fn insert_plain_folder(app: &TestApp, tenant_id: Uuid, created_by: Uuid) -> Uuid {
    let folder_id = Uuid::new_v4();
    drive_folders::ActiveModel {
        id: Set(folder_id),
        name: Set("plain".into()),
        parent_id: Set(None),
        tenant_id: Set(tenant_id),
        project_id: Set(None),
        created_by: Set(created_by),
        created_at: Set(chrono::Utc::now().into()),
    }
    .insert(&app.state.db)
    .await
    .expect("insert plain folder");
    folder_id
}

async fn insert_file(app: &TestApp, tenant_id: Uuid, folder_id: Uuid, uploader: Uuid) -> Uuid {
    let now = chrono::Utc::now();
    let folder = drive_folders::Entity::find_by_id(folder_id)
        .one(&app.state.db)
        .await
        .expect("load folder")
        .expect("folder exists");
    let file_id = Uuid::new_v4();
    drive_files::ActiveModel {
        id: Set(file_id),
        name: Set("note.txt".into()),
        size: Set(5),
        mime_type: Set("text/plain".into()),
        storage_type: Set(drive_files::StorageType::Local),
        storage_key: Set(Uuid::new_v4().to_string()),
        tenant_id: Set(tenant_id),
        project_id: Set(folder.project_id),
        uploader_id: Set(uploader),
        folder_id: Set(Some(folder_id)),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
    }
    .insert(&app.state.db)
    .await
    .expect("insert file");
    file_id
}

async fn add_member(app: &TestApp, project_id: Uuid, user_id: Uuid) {
    common::ensure_tenant_member_for_project(&app.state.db, project_id, user_id).await;
    project_members::ActiveModel {
        id: Set(Uuid::new_v4()),
        project_id: Set(project_id),
        user_id: Set(user_id),
        role: Set(project_members::ProjectRole::Member),
    }
    .insert(&app.state.db)
    .await
    .expect("insert project member");
}

async fn folder_project_id(app: &TestApp, folder_id: Uuid) -> Option<Uuid> {
    drive_folders::Entity::find_by_id(folder_id)
        .one(&app.state.db)
        .await
        .expect("load folder")
        .expect("folder exists")
        .project_id
}

async fn file_project_id(app: &TestApp, file_id: Uuid) -> Option<Uuid> {
    drive_files::Entity::find_by_id(file_id)
        .one(&app.state.db)
        .await
        .expect("load file")
        .expect("file exists")
        .project_id
}

fn folders_url(app: &TestApp, tenant_id: Uuid) -> String {
    format!("{}/v1/tenants/{tenant_id}/drive/folders", app.base_url())
}

/// プロジェクトフォルダの下に作った子フォルダは `project_id` を継承する。
#[tokio::test]
async fn a_child_of_a_project_folder_inherits_the_project() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;
    let root = insert_project_root_folder(&app, tp.tenant_id, tp.project_id, owner.id).await;

    app.reset_session_client();
    app.login_session_no_content(&owner.email, &owner.password)
        .await;

    let response = app
        .client()
        .post(folders_url(&app, tp.tenant_id))
        .json(&serde_json::json!({ "name": "child", "parent_id": root }))
        .send()
        .await
        .expect("create folder");
    assert_eq!(response.status(), StatusCode::CREATED);
    let body: serde_json::Value = response.json().await.expect("json");
    let child_id: Uuid = body["id"].as_str().expect("id").parse().expect("uuid");

    assert_eq!(
        folder_project_id(&app, child_id).await,
        Some(tp.project_id),
        "継承しないと、この中のファイルが一般ファイル扱いになり非メンバーへ漏れる"
    );

    // 対照: 親なしで作ったフォルダは一般フォルダのまま
    let response = app
        .client()
        .post(folders_url(&app, tp.tenant_id))
        .json(&serde_json::json!({ "name": "top" }))
        .send()
        .await
        .expect("create folder");
    let body: serde_json::Value = response.json().await.expect("json");
    let top_id: Uuid = body["id"].as_str().expect("id").parse().expect("uuid");
    assert_eq!(folder_project_id(&app, top_id).await, None);
}

/// 入れないプロジェクトのフォルダ配下には子フォルダを作れない。
#[tokio::test]
async fn creating_a_child_under_a_foreign_project_folder_is_forbidden() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;
    let project_b = insert_extra_project(&app, tp.tenant_id).await;
    let root_b = insert_project_root_folder(&app, tp.tenant_id, project_b, owner.id).await;

    // B をメンバー指定して絞り込み状態にする（#568: 未指定はテナント全体に開放）
    let member = app.insert_user(false, false).await;
    add_member(&app, project_b, member.id).await;

    let attacker = app.insert_user(false, false).await;
    add_member(&app, tp.project_id, attacker.id).await;

    app.reset_session_client();
    app.login_session_no_content(&attacker.email, &attacker.password)
        .await;
    let forbidden = app
        .client()
        .post(folders_url(&app, tp.tenant_id))
        .json(&serde_json::json!({ "name": "child", "parent_id": root_b }))
        .send()
        .await
        .expect("create folder");
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

    // 対照: B のメンバーは作れる（過剰拒否でない）
    app.reset_session_client();
    app.login_session_no_content(&member.email, &member.password)
        .await;
    let created = app
        .client()
        .post(folders_url(&app, tp.tenant_id))
        .json(&serde_json::json!({ "name": "child", "parent_id": root_b }))
        .send()
        .await
        .expect("create folder");
    assert_eq!(created.status(), StatusCode::CREATED);
}

/// フォルダの作成と移動はテナント単位で直列化する。
///
/// 移動をトランザクションへ入れるだけでは足りない。ACL と親の `project_id` を読んでから
/// 書くまでの間に別のリクエストが割り込めるので、一般フォルダをプロジェクト配下へ
/// 移動している最中にそのフォルダへ子を作ると、子は移動前の `project_id = NULL` を
/// 継承したまま残り、直したはずの ACL 漏れが再発する。
///
/// ここではテスト側が同じ鍵でロックを握り、握っているあいだ作成が進まないこと・
/// 離した後に親の値を継承して完了することを見る。
#[tokio::test]
async fn creating_a_folder_waits_for_the_tenant_drive_lock() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;
    let root = insert_project_root_folder(&app, tp.tenant_id, tp.project_id, owner.id).await;

    app.reset_session_client();
    app.login_session_no_content(&owner.email, &owner.password)
        .await;

    // 同じ鍵をテスト側のトランザクションで握る
    let blocker = app.state.db.begin().await.expect("begin blocker");
    common::execute_advisory_lock(
        &blocker,
        service::drive::tenant_drive_lock_key(tp.tenant_id),
    )
    .await;

    let url = folders_url(&app, tp.tenant_id);
    let client = app.client().clone();
    let pending = tokio::spawn(async move {
        client
            .post(url)
            .json(&serde_json::json!({ "name": "child", "parent_id": root }))
            .send()
            .await
            .expect("create folder")
    });

    // 握っているあいだは進まない（ロックが無いと即座に 201 が返る）
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(
        !pending.is_finished(),
        "テナントの Drive ロックを待たずに作成が通っている"
    );

    blocker.rollback().await.expect("release blocker");

    let response = tokio::time::timeout(std::time::Duration::from_secs(10), pending)
        .await
        .expect("ロック解放後に完了する")
        .expect("join");
    assert_eq!(response.status(), StatusCode::CREATED);
    let body: serde_json::Value = response.json().await.expect("json");
    assert_eq!(
        body["project_id"].as_str().map(str::to_string),
        Some(tp.project_id.to_string()),
        "ロック取得後に読んだ親の project_id を継承する"
    );
}

/// フォルダの削除もテナント単位で直列化する。
///
/// 子の有無を読んでから消すまでの間に子を作られると、`parent_id` の ON DELETE SET NULL で
/// その子が `parent_id = NULL` かつ `project_id = Some` のままドライブ直下へ出る。これは
/// 自動生成のプロジェクトルートと同じ形なので、以後は更新も削除も 409 で拒まれ、API から
/// 片付けられなくなる（`drive_files` と違い、この表には受け止める CHECK が無い）。
///
/// ここではテスト側が同じ鍵でロックを握り、握っているあいだ削除が進まないこと・
/// 離した後に完了することを見る。
#[tokio::test]
async fn deleting_a_folder_waits_for_the_tenant_drive_lock() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;
    let plain = insert_plain_folder(&app, tp.tenant_id, owner.id).await;

    app.reset_session_client();
    app.login_session_no_content(&owner.email, &owner.password)
        .await;

    // 同じ鍵をテスト側のトランザクションで握る
    let blocker = app.state.db.begin().await.expect("begin blocker");
    common::execute_advisory_lock(
        &blocker,
        service::drive::tenant_drive_lock_key(tp.tenant_id),
    )
    .await;

    let url = format!("{}/{plain}", folders_url(&app, tp.tenant_id));
    let client = app.client().clone();
    let pending =
        tokio::spawn(async move { client.delete(url).send().await.expect("delete folder") });

    // 握っているあいだは進まない（ロックが無いと即座に 204 が返る）
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(
        !pending.is_finished(),
        "テナントの Drive ロックを待たずに削除が通っている"
    );

    blocker.rollback().await.expect("release blocker");

    let response = tokio::time::timeout(std::time::Duration::from_secs(10), pending)
        .await
        .expect("ロック解放後に完了する")
        .expect("join");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

/// フォルダ移動で配下のフォルダ・ファイルの `project_id` が揃う。
#[tokio::test]
async fn moving_a_folder_syncs_the_project_of_everything_underneath() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;
    let root = insert_project_root_folder(&app, tp.tenant_id, tp.project_id, owner.id).await;
    let plain = insert_plain_folder(&app, tp.tenant_id, owner.id).await;

    app.reset_session_client();
    app.login_session_no_content(&owner.email, &owner.password)
        .await;

    // 一般フォルダの下に 2 段の階層とファイルを作る
    let mut ids = Vec::new();
    let mut parent = plain;
    for name in ["mid", "leaf"] {
        let response = app
            .client()
            .post(folders_url(&app, tp.tenant_id))
            .json(&serde_json::json!({ "name": name, "parent_id": parent }))
            .send()
            .await
            .expect("create folder");
        let body: serde_json::Value = response.json().await.expect("json");
        parent = body["id"].as_str().expect("id").parse().expect("uuid");
        ids.push(parent);
    }
    let leaf_file = insert_file(&app, tp.tenant_id, parent, owner.id).await;
    assert_eq!(file_project_id(&app, leaf_file).await, None);

    // 一般フォルダをプロジェクトフォルダの下へ移す
    let moved = app
        .client()
        .patch(format!("{}/{plain}", folders_url(&app, tp.tenant_id)))
        .json(&serde_json::json!({ "parent_id": root }))
        .send()
        .await
        .expect("move folder");
    assert_eq!(moved.status(), StatusCode::OK);
    let body: serde_json::Value = moved.json().await.expect("json");
    assert_eq!(
        body["project_id"].as_str().map(str::to_string),
        Some(tp.project_id.to_string()),
        "レスポンスも同期後の project_id を返す"
    );

    assert_eq!(folder_project_id(&app, plain).await, Some(tp.project_id));
    for id in &ids {
        assert_eq!(
            folder_project_id(&app, *id).await,
            Some(tp.project_id),
            "配下フォルダが取り残されると階層と認可が食い違う"
        );
    }
    assert_eq!(file_project_id(&app, leaf_file).await, Some(tp.project_id));

    // 戻すと一般ファイルへ戻る（片道だけ同期していないこと）
    let moved_back = app
        .client()
        .patch(format!("{}/{plain}", folders_url(&app, tp.tenant_id)))
        .json(&serde_json::json!({ "parent_id": serde_json::Value::Null }))
        .send()
        .await
        .expect("move folder back");
    assert_eq!(moved_back.status(), StatusCode::OK);
    assert_eq!(folder_project_id(&app, plain).await, None);
    assert_eq!(file_project_id(&app, leaf_file).await, None);
}

/// 入れないプロジェクトへフォルダを送り込めない／そこから持ち出せない。
#[tokio::test]
async fn moving_across_a_project_boundary_is_forbidden() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;
    let project_b = insert_extra_project(&app, tp.tenant_id).await;
    let root_b = insert_project_root_folder(&app, tp.tenant_id, project_b, owner.id).await;
    let member = app.insert_user(false, false).await;
    add_member(&app, project_b, member.id).await;

    let attacker = app.insert_user(false, false).await;
    add_member(&app, tp.project_id, attacker.id).await;

    app.reset_session_client();
    app.login_session_no_content(&attacker.email, &attacker.password)
        .await;

    // 自分の一般フォルダを B のフォルダ配下へ送り込む
    let response = app
        .client()
        .post(folders_url(&app, tp.tenant_id))
        .json(&serde_json::json!({ "name": "mine" }))
        .send()
        .await
        .expect("create folder");
    let body: serde_json::Value = response.json().await.expect("json");
    let mine: Uuid = body["id"].as_str().expect("id").parse().expect("uuid");

    let into_b = app
        .client()
        .patch(format!("{}/{mine}", folders_url(&app, tp.tenant_id)))
        .json(&serde_json::json!({ "parent_id": root_b }))
        .send()
        .await
        .expect("move into b");
    assert_eq!(
        into_b.status(),
        StatusCode::FORBIDDEN,
        "移動先の ACL を見ないと他人のプロジェクトへ置ける"
    );

    // B 配下のフォルダを持ち出す（移動元の ACL）
    app.reset_session_client();
    app.login_session_no_content(&member.email, &member.password)
        .await;
    let response = app
        .client()
        .post(folders_url(&app, tp.tenant_id))
        .json(&serde_json::json!({ "name": "inside-b", "parent_id": root_b }))
        .send()
        .await
        .expect("create folder");
    let body: serde_json::Value = response.json().await.expect("json");
    let inside_b: Uuid = body["id"].as_str().expect("id").parse().expect("uuid");

    app.reset_session_client();
    app.login_session_no_content(&attacker.email, &attacker.password)
        .await;
    let out_of_b = app
        .client()
        .patch(format!("{}/{inside_b}", folders_url(&app, tp.tenant_id)))
        .json(&serde_json::json!({ "parent_id": serde_json::Value::Null }))
        .send()
        .await
        .expect("move out of b");
    assert_eq!(
        out_of_b.status(),
        StatusCode::FORBIDDEN,
        "移動元の ACL を見ないと他人のプロジェクトから持ち出せる"
    );
}

/// 入れないプロジェクトのフォルダへファイルを送り込めない。
#[tokio::test]
async fn moving_a_file_into_a_foreign_project_folder_is_forbidden() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;
    let project_b = insert_extra_project(&app, tp.tenant_id).await;
    let root_b = insert_project_root_folder(&app, tp.tenant_id, project_b, owner.id).await;
    let member = app.insert_user(false, false).await;
    add_member(&app, project_b, member.id).await;

    let attacker = app.insert_user(false, false).await;
    add_member(&app, tp.project_id, attacker.id).await;
    let plain = insert_plain_folder(&app, tp.tenant_id, attacker.id).await;
    let file = insert_file(&app, tp.tenant_id, plain, attacker.id).await;

    app.reset_session_client();
    app.login_session_no_content(&attacker.email, &attacker.password)
        .await;
    let forbidden = app
        .client()
        .patch(format!(
            "{}/v1/tenants/{}/drive/files/{file}",
            app.base_url(),
            tp.tenant_id
        ))
        .json(&serde_json::json!({ "folder_id": root_b }))
        .send()
        .await
        .expect("move file");
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

    // 対照: B のメンバーなら同じ移動ができる
    app.reset_session_client();
    app.login_session_no_content(&member.email, &member.password)
        .await;
    let allowed = app
        .client()
        .patch(format!(
            "{}/v1/tenants/{}/drive/files/{file}",
            app.base_url(),
            tp.tenant_id
        ))
        .json(&serde_json::json!({ "folder_id": root_b }))
        .send()
        .await
        .expect("move file");
    assert_eq!(allowed.status(), StatusCode::OK);
    assert_eq!(file_project_id(&app, file).await, Some(project_b));
}

/// 自動生成のプロジェクトルートフォルダは、空でも削除・移動できない。
#[tokio::test]
async fn the_project_root_folder_cannot_be_deleted_or_moved() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;
    let root = insert_project_root_folder(&app, tp.tenant_id, tp.project_id, owner.id).await;
    let plain = insert_plain_folder(&app, tp.tenant_id, owner.id).await;

    app.reset_session_client();
    app.login_session_no_content(&owner.email, &owner.password)
        .await;

    // 中身が無くても消せない（プロジェクト削除の CASCADE に任せる）
    let deleted = app
        .client()
        .delete(format!("{}/{root}", folders_url(&app, tp.tenant_id)))
        .send()
        .await
        .expect("delete root");
    assert_eq!(deleted.status(), StatusCode::CONFLICT);

    let moved = app
        .client()
        .patch(format!("{}/{root}", folders_url(&app, tp.tenant_id)))
        .json(&serde_json::json!({ "parent_id": plain }))
        .send()
        .await
        .expect("move root");
    assert_eq!(moved.status(), StatusCode::CONFLICT);

    // 対照: 名前の変更は通る（ルートを全面凍結していない）
    let renamed = app
        .client()
        .patch(format!("{}/{root}", folders_url(&app, tp.tenant_id)))
        .json(&serde_json::json!({ "name": "renamed" }))
        .send()
        .await
        .expect("rename root");
    assert_eq!(renamed.status(), StatusCode::OK);

    // 対照: 普通のフォルダは空なら消せる
    let deleted_plain = app
        .client()
        .delete(format!("{}/{plain}", folders_url(&app, tp.tenant_id)))
        .send()
        .await
        .expect("delete plain");
    assert_eq!(deleted_plain.status(), StatusCode::NO_CONTENT);
}

/// ロックを待っているあいだにファイルが別プロジェクトへ移ったら、更新は拒む。
///
/// 取得と認可をロックの外で済ませると、ロックを待っているあいだに別のリクエストが
/// そのファイルを自分の入れないプロジェクトへ移していても、古いモデルと古い認可結果の
/// まま名前を変えたりルートへ持ち出したりできる。ここではテスト側が同じ鍵でロックを
/// 握り、握っているあいだに移動を確定させてから離す。
#[tokio::test]
async fn updating_a_file_re_authorizes_after_waiting_for_the_lock() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;
    // 書き手はプロジェクト A のメンバー。B には入れない
    let writer = app.insert_user(false, false).await;
    add_member(&app, tp.project_id, writer.id).await;
    let project_b = insert_extra_project(&app, tp.tenant_id).await;
    // メンバーが 1 人も居ないプロジェクトはテナント全体へ開くので（project_is_open_or_member）、
    // 別の利用者を入れて閉じた状態にする。そうしないと writer も入れてしまう
    let stranger = app.insert_user(false, false).await;
    add_member(&app, project_b, stranger.id).await;

    let root_a = insert_project_root_folder(&app, tp.tenant_id, tp.project_id, owner.id).await;
    let file = insert_file(&app, tp.tenant_id, root_a, owner.id).await;

    app.reset_session_client();
    app.login_session_no_content(&writer.email, &writer.password)
        .await;

    let file_url = format!(
        "{}/v1/tenants/{}/drive/files/{file}",
        app.base_url(),
        tp.tenant_id
    );

    // 対照: 移動していなければ同じ要求が通る（過剰拒否になっていないこと）
    let allowed = app
        .client()
        .patch(&file_url)
        .json(&serde_json::json!({ "name": "before.txt" }))
        .send()
        .await
        .expect("rename file");
    assert_eq!(allowed.status(), StatusCode::OK);

    // 同じ鍵をテスト側のトランザクションで握る
    let blocker = app.state.db.begin().await.expect("begin blocker");
    common::execute_advisory_lock(
        &blocker,
        service::drive::tenant_drive_lock_key(tp.tenant_id),
    )
    .await;

    let client = app.client().clone();
    let pending_url = file_url.clone();
    let pending = tokio::spawn(async move {
        client
            .patch(pending_url)
            .json(&serde_json::json!({ "name": "renamed-after-move.txt" }))
            .send()
            .await
            .expect("rename file")
    });

    // 握っているあいだは進まない（ロックが無いと即座に 200 が返る）
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(
        !pending.is_finished(),
        "テナントの Drive ロックを待たずに更新が通っている"
    );

    // ロックを握ったまま、書き手が入れないプロジェクトへ移して確定させる
    common::execute_sql(
        &blocker,
        "UPDATE drive_files SET project_id = $1 WHERE id = $2",
        vec![project_b.into(), file.into()],
    )
    .await;
    blocker.commit().await.expect("commit blocker");

    let response = tokio::time::timeout(std::time::Duration::from_secs(10), pending)
        .await
        .expect("ロック解放後に完了する")
        .expect("join");
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "ロック待ちのあいだに移ったファイルを、古い認可結果のまま更新できている"
    );

    // 名前も変わっていない（ロックの内側で弾けている）
    let after = drive_files::Entity::find_by_id(file)
        .one(&app.state.db)
        .await
        .expect("load file")
        .expect("file exists");
    assert_eq!(after.name, "before.txt");
    assert_eq!(after.project_id, Some(project_b));
}
