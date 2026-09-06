// Drive の行を作るテストは、backfill のテスト（`drive_project_id_backfill_integration`）が
// 実行前に `TRUNCATE drive_files, drive_folders CASCADE` を流すのと同じ鍵で直列化する。
// backfill の SQL はテナントを跨いで全行を見るので、あちらは Drive を空にしてからでないと
// 他のファイルの残骸で落ちる。鍵を共有しないと、その TRUNCATE がこちらの実行中の行を
// 巻き添えにする（CASCADE は drive_folder_shares と task_attachments にも及ぶ）。

mod common;

use axum::http::StatusCode;
use common::TestApp;
use reqwest::multipart::{Form, Part};
use uuid::Uuid;

/// ログイン済みセッションでテナント直下にファイルをアップロードする。
async fn upload_text_file(app: &TestApp, tenant_id: Uuid, file_name: &str, body: &[u8]) {
    let form = Form::new().part(
        "file",
        Part::bytes(body.to_vec())
            .file_name(file_name.to_string())
            .mime_str("text/plain")
            .expect("mime"),
    );

    let response = app
        .client()
        .post(format!(
            "{}/v1/tenants/{tenant_id}/drive/files",
            app.base_url()
        ))
        .multipart(form)
        .send()
        .await
        .expect("upload request");
    assert_eq!(
        response.status(),
        StatusCode::CREATED,
        "{file_name} のアップロードは成功するはず"
    );
}

/// テナントに複数ファイルがある状態で使用量を集計できること。
///
/// Postgres の `SUM(bigint)` は NUMERIC を返すため、`tenant_used_bytes` が
/// そのまま `i64` で受け取っていた頃はデコードに失敗していた。行が 0 件のときは
/// SUM が NULL になり素通りするので 1 件目までは成功し、2 件目のアップロード
/// （クォータ事前チェックで使用量を読む）と使用量取得が 500 になっていた。
#[tokio::test]
#[serial_test::file_serial(drive)]
async fn drive_usage_sums_multiple_files() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;
    app.reset_session_client();
    app.login_session_no_content(&owner.email, &owner.password)
        .await;

    upload_text_file(&app, tp.tenant_id, "one.txt", b"12345").await;
    // 2 件目はクォータ事前チェックで tenant_used_bytes を呼ぶ経路に入る。
    upload_text_file(&app, tp.tenant_id, "two.txt", b"1234567890").await;

    let usage = app
        .get_with_session(&format!("/v1/tenants/{}/drive/usage", tp.tenant_id))
        .await;
    assert_eq!(usage.status(), StatusCode::OK, "使用量取得は成功するはず");

    let body: serde_json::Value = usage.json().await.expect("usage json");
    assert_eq!(
        body["used_bytes"].as_i64(),
        Some(15),
        "複数ファイルのサイズが合算される"
    );

    app.cleanup_user(owner.id).await;
}
