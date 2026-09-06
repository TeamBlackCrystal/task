// Drive の行を作るテストは、backfill のテスト（`drive_project_id_backfill_integration`）が
// 実行前に `TRUNCATE drive_files, drive_folders CASCADE` を流すのと同じ鍵で直列化する。
// backfill の SQL はテナントを跨いで全行を見るので、あちらは Drive を空にしてからでないと
// 他のファイルの残骸で落ちる。鍵を共有しないと、その TRUNCATE がこちらの実行中の行を
// 巻き添えにする（CASCADE は drive_folder_shares と task_attachments にも及ぶ）。

mod common;

use axum::http::StatusCode;
use common::TestApp;
use entity::{drive_files, drive_folder_shares, drive_folders, users};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait};
use uuid::Uuid;

/// 公開共有の 2 エンドポイントは仕様上 `/v1/drive/share/{token}` に居る。
///
/// routes 側が `public_routes()` を `/drive` へ nest しているのに、ハンドラーが
/// `path = "/v1/drive/share/{token}"` と絶対で宣言していたため、実際の登録と OpenAPI が
/// `/v1/drive/v1/drive/share/{token}` になっていた（#277 と同じ踏み方）。
/// 仕様の URL で引けることを固定して、絶対パスへ戻す退行を落とす。
const SHARE_BASE: &str = "/v1/drive/share";

async fn insert_folder(app: &TestApp, tenant_id: Uuid, created_by: Uuid) -> Uuid {
    let folder_id = Uuid::new_v4();
    drive_folders::ActiveModel {
        id: Set(folder_id),
        name: Set("shared-folder".into()),
        parent_id: Set(None),
        tenant_id: Set(tenant_id),
        project_id: Set(None),
        created_by: Set(created_by),
        created_at: Set(chrono::Utc::now().into()),
    }
    .insert(&app.state.db)
    .await
    .expect("insert folder");
    folder_id
}

async fn insert_file(app: &TestApp, tenant_id: Uuid, folder_id: Uuid, uploader: Uuid, name: &str) {
    let now = chrono::Utc::now();
    drive_files::ActiveModel {
        id: Set(Uuid::new_v4()),
        name: Set(name.into()),
        size: Set(5),
        mime_type: Set("text/plain".into()),
        storage_type: Set(drive_files::StorageType::Local),
        storage_key: Set(Uuid::new_v4().to_string()),
        tenant_id: Set(tenant_id),
        project_id: Set(None),
        uploader_id: Set(uploader),
        folder_id: Set(Some(folder_id)),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
    }
    .insert(&app.state.db)
    .await
    .expect("insert file");
}

/// 公開リンク共有を 1 件作る。`expires_at` に過去を入れると期限切れになる。
async fn insert_public_share(
    app: &TestApp,
    folder_id: Uuid,
    created_by: Uuid,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
) -> String {
    let token = format!("tok{}", Uuid::new_v4().simple());
    drive_folder_shares::ActiveModel {
        id: Set(Uuid::new_v4()),
        folder_id: Set(folder_id),
        shared_with_user_id: Set(None),
        share_token: Set(Some(token.clone())),
        permission: Set(drive_folder_shares::SharePermission::Viewer),
        created_by: Set(created_by),
        expires_at: Set(expires_at.map(Into::into)),
        created_at: Set(chrono::Utc::now().into()),
    }
    .insert(&app.state.db)
    .await
    .expect("insert public share");
    token
}

/// 仕様の URL でフォルダメタデータとファイル一覧が引ける（認証不要）。
#[tokio::test]
#[serial_test::file_serial(drive)]
async fn public_share_endpoints_live_at_the_documented_paths() {
    let app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;
    let folder = insert_folder(&app, tp.tenant_id, owner.id).await;
    insert_file(&app, tp.tenant_id, folder, owner.id, "a.txt").await;
    insert_file(&app, tp.tenant_id, folder, owner.id, "b.txt").await;
    let token = insert_public_share(&app, folder, owner.id, None).await;

    // 認証しないクライアントで引く（公開リンクはセッションも PAT も要らない）
    let folder_response = reqwest::Client::new()
        .get(format!("{}{SHARE_BASE}/{token}", app.base_url()))
        .send()
        .await
        .expect("folder request");
    assert_eq!(
        folder_response.status(),
        StatusCode::OK,
        "仕様の URL でフォルダメタデータが引ける"
    );
    let body: serde_json::Value = folder_response.json().await.expect("folder json");
    assert_eq!(body["name"], "shared-folder");
    assert_eq!(body["file_count"], 2);
    // 作成者名は users から解決される（TestUser は username を持たないので DB から引く）
    let owner_name = users::Entity::find_by_id(owner.id)
        .one(&app.state.db)
        .await
        .expect("load owner")
        .expect("owner exists")
        .username;
    assert_eq!(body["created_by_name"], owner_name);

    let files_response = reqwest::Client::new()
        .get(format!("{}{SHARE_BASE}/{token}/files", app.base_url()))
        .send()
        .await
        .expect("files request");
    assert_eq!(
        files_response.status(),
        StatusCode::OK,
        "仕様の URL でファイル一覧が引ける"
    );
    let files: serde_json::Value = files_response.json().await.expect("files json");
    let mut names: Vec<String> = files
        .as_array()
        .expect("array")
        .iter()
        .map(|file| file["name"].as_str().expect("name").to_string())
        .collect();
    names.sort();
    assert_eq!(names, vec!["a.txt".to_string(), "b.txt".to_string()]);
}

/// 二重 prefix の URL は登録されていない（直したことの裏返しを固定する）。
#[tokio::test]
#[serial_test::file_serial(drive)]
async fn the_double_prefixed_path_is_gone() {
    let app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;
    let folder = insert_folder(&app, tp.tenant_id, owner.id).await;
    let token = insert_public_share(&app, folder, owner.id, None).await;

    let response = reqwest::Client::new()
        .get(format!(
            "{}/v1/drive/v1/drive/share/{token}",
            app.base_url()
        ))
        .send()
        .await
        .expect("legacy path request");
    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "二重 prefix の URL には何も居ない"
    );
}

/// 存在しないトークンは 404。
#[tokio::test]
#[serial_test::file_serial(drive)]
async fn unknown_token_is_not_found() {
    let app = TestApp::new().await;

    for path in [
        format!("{}{SHARE_BASE}/does-not-exist", app.base_url()),
        format!("{}{SHARE_BASE}/does-not-exist/files", app.base_url()),
    ] {
        let response = reqwest::Client::new()
            .get(&path)
            .send()
            .await
            .expect("request");
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

/// 期限切れトークンは 410 Gone（仕様 §8.3）。期限ちょうど手前は通ることも見る。
#[tokio::test]
#[serial_test::file_serial(drive)]
async fn expired_token_is_gone_but_a_live_one_still_works() {
    let app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;
    let folder = insert_folder(&app, tp.tenant_id, owner.id).await;

    let expired = insert_public_share(
        &app,
        folder,
        owner.id,
        Some(chrono::Utc::now() - chrono::Duration::minutes(1)),
    )
    .await;
    for path in [
        format!("{}{SHARE_BASE}/{expired}", app.base_url()),
        format!("{}{SHARE_BASE}/{expired}/files", app.base_url()),
    ] {
        let response = reqwest::Client::new()
            .get(&path)
            .send()
            .await
            .expect("request");
        assert_eq!(response.status(), StatusCode::GONE, "{path}");
    }

    // 対照: まだ期限内の共有は通る（期限判定で過剰に閉じていない）
    let live = insert_public_share(
        &app,
        folder,
        owner.id,
        Some(chrono::Utc::now() + chrono::Duration::hours(1)),
    )
    .await;
    let response = reqwest::Client::new()
        .get(format!("{}{SHARE_BASE}/{live}", app.base_url()))
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), StatusCode::OK, "期限内の共有は引ける");
}
