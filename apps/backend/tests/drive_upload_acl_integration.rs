// Drive の行を作るテストは、backfill のテスト（`drive_project_id_backfill_integration`）が
// 実行前に `TRUNCATE drive_files, drive_folders CASCADE` を流すのと同じ鍵で直列化する。
// backfill の SQL はテナントを跨いで全行を見るので、あちらは Drive を空にしてからでないと
// 他のファイルの残骸で落ちる。鍵を共有しないと、その TRUNCATE がこちらの実行中の行を
// 巻き添えにする（CASCADE は drive_folder_shares と task_attachments にも及ぶ）。

mod common;

use axum::http::StatusCode;
use common::TestApp;
use entity::{drive_folders, project_members, projects};
use reqwest::multipart::{Form, Part};
use sea_orm::{ActiveModelTrait, ActiveValue::Set};
use uuid::Uuid;

/// 同一テナント内に 2 つ目のプロジェクトを差し込む（insert_tenant_project は 1 つだけ作る）。
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
    .expect("insert second project");
    project_id
}

async fn insert_project_folder(
    app: &TestApp,
    tenant_id: Uuid,
    project_id: Uuid,
    created_by: Uuid,
) -> Uuid {
    let folder_id = Uuid::new_v4();
    drive_folders::ActiveModel {
        id: Set(folder_id),
        name: Set("project-folder".into()),
        parent_id: Set(None),
        tenant_id: Set(tenant_id),
        project_id: Set(Some(project_id)),
        created_by: Set(created_by),
        created_at: Set(chrono::Utc::now().into()),
    }
    .insert(&app.state.db)
    .await
    .expect("insert project folder");
    folder_id
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

fn upload_form(folder_id: Uuid) -> Form {
    // ハンドラは 'file' より前に 'folder_id' を読む前提。reqwest は挿入順を保持する。
    Form::new().text("folder_id", folder_id.to_string()).part(
        "file",
        Part::bytes(b"hello".to_vec())
            .file_name("hello.txt")
            .mime_str("text/plain")
            .expect("mime"),
    )
}

async fn upload_status(app: &TestApp, tenant_id: Uuid, folder_id: Uuid) -> StatusCode {
    app.client()
        .post(format!(
            "{}/v1/tenants/{tenant_id}/drive/files",
            app.base_url()
        ))
        .multipart(upload_form(folder_id))
        .send()
        .await
        .expect("upload request")
        .status()
}

/// プロジェクト B のフォルダへのアップロードは、B の非メンバーには 403。
/// メンバーには 201。プロジェクトファイルの読み取り/更新/削除と同じ ACL を作成にも適用する。
#[tokio::test]
#[serial_test::file_serial(drive)]
async fn upload_into_project_folder_enforces_membership() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await; // tenant T + project A
    let project_b = insert_extra_project(&app, tp.tenant_id).await;
    let folder_b = insert_project_folder(&app, tp.tenant_id, project_b, owner.id).await;

    // プロジェクト B にメンバーを 1 人指定して「絞り込み状態」にする。
    // #568 でメンバー未指定のプロジェクトはテナント全体に開放されるため、
    // プロジェクト単位の隔離を試すには先に指定が要る。
    let member = app.insert_user(false, false).await;
    add_member(&app, project_b, member.id).await;

    // 攻撃者: プロジェクト A のメンバー（テナントアクセスは通る）だが B の非メンバー。
    let attacker = app.insert_user(false, false).await;
    add_member(&app, tp.project_id, attacker.id).await;

    app.reset_session_client();
    app.login_session_no_content(&attacker.email, &attacker.password)
        .await;
    let forbidden = upload_status(&app, tp.tenant_id, folder_b).await;
    assert_eq!(
        forbidden,
        StatusCode::FORBIDDEN,
        "B 非メンバーは B のフォルダへアップロードできない"
    );

    // 対照: プロジェクト B のメンバーは同じフォルダへアップロードできる（過剰拒否でない）。
    app.reset_session_client();
    app.login_session_no_content(&member.email, &member.password)
        .await;
    let created = upload_status(&app, tp.tenant_id, folder_b).await;
    assert_eq!(
        created,
        StatusCode::CREATED,
        "B メンバーは B のフォルダへアップロードできる"
    );

    // owner を先に削除するとテナント配下（プロジェクト・フォルダ・ファイル・メンバー）が
    // カスケード削除され、残りのユーザー削除で FK が衝突しない。
    app.cleanup_user(owner.id).await;
    app.cleanup_user(attacker.id).await;
    app.cleanup_user(member.id).await;
}
