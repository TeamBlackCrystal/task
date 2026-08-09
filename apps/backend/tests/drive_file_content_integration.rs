mod common;

use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use common::TestApp;
use entity::{drive_folders, project_members, projects, tenants};
use reqwest::multipart::{Form, Part};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait};
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
        created_at: Set(Utc::now().into()),
    }
    .insert(&app.state.db)
    .await
    .expect("insert project folder");
    folder_id
}

async fn add_member(app: &TestApp, project_id: Uuid, user_id: Uuid) {
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

async fn set_tenant_quota(app: &TestApp, tenant_id: Uuid, quota_bytes: i64) {
    let tenant = tenants::Entity::find_by_id(tenant_id)
        .one(&app.state.db)
        .await
        .expect("load tenant")
        .expect("tenant exists");
    let mut active: tenants::ActiveModel = tenant.into();
    active.drive_quota_bytes = Set(Some(quota_bytes));
    active
        .update(&app.state.db)
        .await
        .expect("update tenant quota");
}

/// ログイン済みセッションでファイルをアップロードし、レスポンス JSON を返す。
async fn upload_file(
    app: &TestApp,
    tenant_id: Uuid,
    folder_id: Option<Uuid>,
    file_name: &str,
    mime: &str,
    body: &[u8],
) -> serde_json::Value {
    // ハンドラは 'file' より前に 'folder_id' を読む前提。reqwest は挿入順を保持する。
    let mut form = Form::new();
    if let Some(folder_id) = folder_id {
        form = form.text("folder_id", folder_id.to_string());
    }
    form = form.part(
        "file",
        Part::bytes(body.to_vec())
            .file_name(file_name.to_string())
            .mime_str(mime)
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
        "アップロードは成功するはず"
    );
    response.json().await.expect("upload response json")
}

fn content_path(tenant_id: Uuid, file_id: Uuid) -> String {
    format!("/v1/tenants/{tenant_id}/drive/files/{file_id}/content")
}

fn file_id_of(file: &serde_json::Value) -> Uuid {
    Uuid::parse_str(file["id"].as_str().expect("id is string")).expect("id is uuid")
}

fn timestamp_of(file: &serde_json::Value, field: &str) -> DateTime<Utc> {
    file[field]
        .as_str()
        .unwrap_or_else(|| panic!("{field} is string"))
        .parse()
        .unwrap_or_else(|_| panic!("{field} is rfc3339"))
}

/// テキストファイルの本文を差し替えられる。size と updated_at が更新され、
/// 配信エンドポイントからも新しい内容が読める。
#[tokio::test]
async fn update_content_replaces_text_file_body() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;
    app.reset_session_client();
    app.login_session_no_content(&owner.email, &owner.password)
        .await;

    let uploaded = upload_file(
        &app,
        tp.tenant_id,
        None,
        "notes.md",
        "text/markdown",
        b"before",
    )
    .await;
    let file_id = file_id_of(&uploaded);
    let created_at = timestamp_of(&uploaded, "created_at");
    assert_eq!(
        uploaded["size"].as_i64(),
        Some(6),
        "アップロード直後のサイズ"
    );

    let new_body = "# after\n\n書き換えた本文";
    let response = app
        .put_json_with_session(
            &content_path(tp.tenant_id, file_id),
            serde_json::json!({ "content": new_body }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK, "本文更新は成功するはず");

    let updated: serde_json::Value = response.json().await.expect("update response json");
    assert_eq!(
        updated["size"].as_i64(),
        Some(new_body.len() as i64),
        "size は新しい本文のバイト数になる"
    );
    assert_eq!(
        updated["mime_type"].as_str(),
        Some("text/markdown"),
        "mime_type は据え置き"
    );
    assert!(
        timestamp_of(&updated, "updated_at") > created_at,
        "updated_at が編集時刻に更新される"
    );

    // 配信エンドポイントから新しい内容が読めること（ストレージにも反映されている）。
    let content = app
        .get_with_session(&format!("/v1/drive/files/{file_id}/content"))
        .await;
    assert_eq!(content.status(), StatusCode::OK);
    assert_eq!(
        content.text().await.expect("content body"),
        new_body,
        "配信される内容が差し替わっている"
    );

    app.cleanup_user(owner.id).await;
}

/// 空文字列での更新は「中身を空にする編集」として許可する。
#[tokio::test]
async fn update_content_allows_empty_body() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;
    app.reset_session_client();
    app.login_session_no_content(&owner.email, &owner.password)
        .await;

    let uploaded = upload_file(&app, tp.tenant_id, None, "memo.txt", "text/plain", b"xyz").await;
    let file_id = file_id_of(&uploaded);

    let response = app
        .put_json_with_session(
            &content_path(tp.tenant_id, file_id),
            serde_json::json!({ "content": "" }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK, "空文字列でも更新できる");
    let updated: serde_json::Value = response.json().await.expect("update response json");
    assert_eq!(updated["size"].as_i64(), Some(0), "size は 0 になる");

    app.cleanup_user(owner.id).await;
}

/// テキストとして扱えない MIME のファイルは編集できない（400）。
/// 対照として同じ経路でテキストファイルは編集できることも確認する。
#[tokio::test]
async fn update_content_rejects_non_text_mime() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;
    app.reset_session_client();
    app.login_session_no_content(&owner.email, &owner.password)
        .await;

    let binary = upload_file(
        &app,
        tp.tenant_id,
        None,
        "logo.png",
        "image/png",
        &[0x89, 0x50, 0x4e, 0x47],
    )
    .await;
    let binary_id = file_id_of(&binary);
    let rejected = app
        .put_json_with_session(
            &content_path(tp.tenant_id, binary_id),
            serde_json::json!({ "content": "not an image" }),
        )
        .await;
    assert_eq!(
        rejected.status(),
        StatusCode::BAD_REQUEST,
        "image/png はテキストとして編集できない"
    );

    // 対照: .ts はブラウザが MPEG-TS として申告してくることがあるが、
    // 拡張子の上書きが優先されるため TypeScript として保存され、編集できる
    // （過剰拒否でないこと）。
    let source = upload_file(
        &app,
        tp.tenant_id,
        None,
        "main.ts",
        "video/mp2t",
        b"export const a = 1\n",
    )
    .await;
    assert_eq!(
        source["mime_type"].as_str(),
        Some("text/typescript"),
        "クライアント申告より拡張子の上書きが優先される"
    );
    let source_id = file_id_of(&source);
    let accepted = app
        .put_json_with_session(
            &content_path(tp.tenant_id, source_id),
            serde_json::json!({ "content": "export const a = 2\n" }),
        )
        .await;
    assert_eq!(
        accepted.status(),
        StatusCode::OK,
        "テキスト系ファイルは編集できる"
    );

    app.cleanup_user(owner.id).await;
}

/// 他テナントのファイル ID を自テナントのパスで指定しても 404（テナント越えを許さない）。
#[tokio::test]
async fn update_content_does_not_cross_tenants() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;
    let other_tenant_id = common::insert_tenant(&app.state.db, owner.id).await;
    app.reset_session_client();
    app.login_session_no_content(&owner.email, &owner.password)
        .await;

    let uploaded = upload_file(&app, tp.tenant_id, None, "memo.txt", "text/plain", b"body").await;
    let file_id = file_id_of(&uploaded);

    let not_found = app
        .put_json_with_session(
            &content_path(other_tenant_id, file_id),
            serde_json::json!({ "content": "leak" }),
        )
        .await;
    assert_eq!(
        not_found.status(),
        StatusCode::NOT_FOUND,
        "別テナント配下としては見つからない"
    );

    // 対照: 正しいテナントのパスなら編集できる。
    let ok = app
        .put_json_with_session(
            &content_path(tp.tenant_id, file_id),
            serde_json::json!({ "content": "edited" }),
        )
        .await;
    assert_eq!(ok.status(), StatusCode::OK, "自テナント配下では編集できる");

    app.cleanup_user(owner.id).await;
}

/// プロジェクトフォルダ内のファイル編集は、そのプロジェクトのメンバーのみ許可（403 / 200）。
#[tokio::test]
async fn update_content_enforces_project_membership() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await; // tenant T + project A
    let project_b = insert_extra_project(&app, tp.tenant_id).await;
    let folder_b = insert_project_folder(&app, tp.tenant_id, project_b, owner.id).await;

    // テナントオーナーとして project B のフォルダにテキストファイルを置く。
    app.reset_session_client();
    app.login_session_no_content(&owner.email, &owner.password)
        .await;
    let uploaded = upload_file(
        &app,
        tp.tenant_id,
        Some(folder_b),
        "shared.txt",
        "text/plain",
        b"original",
    )
    .await;
    let file_id = file_id_of(&uploaded);

    // 攻撃者: project A のメンバー（テナントアクセスは通る）だが B の非メンバー。
    let attacker = app.insert_user(false, false).await;
    add_member(&app, tp.project_id, attacker.id).await;
    app.reset_session_client();
    app.login_session_no_content(&attacker.email, &attacker.password)
        .await;
    let forbidden = app
        .put_json_with_session(
            &content_path(tp.tenant_id, file_id),
            serde_json::json!({ "content": "tampered" }),
        )
        .await;
    assert_eq!(
        forbidden.status(),
        StatusCode::FORBIDDEN,
        "B 非メンバーは B のファイルを編集できない"
    );

    // 対照: project B のメンバーは編集できる（過剰拒否でない）。
    let member = app.insert_user(false, false).await;
    add_member(&app, project_b, member.id).await;
    app.reset_session_client();
    app.login_session_no_content(&member.email, &member.password)
        .await;
    let allowed = app
        .put_json_with_session(
            &content_path(tp.tenant_id, file_id),
            serde_json::json!({ "content": "edited by member" }),
        )
        .await;
    assert_eq!(
        allowed.status(),
        StatusCode::OK,
        "B メンバーは B のファイルを編集できる"
    );

    // owner を先に削除するとテナント配下がカスケード削除され、残りの削除で FK が衝突しない。
    app.cleanup_user(owner.id).await;
    app.cleanup_user(attacker.id).await;
    app.cleanup_user(member.id).await;
}

/// 差し替えでクォータを超える場合は 413。収まる場合は 200（旧サイズを差し引いて判定している）。
#[tokio::test]
async fn update_content_enforces_quota_on_size_delta() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;
    app.reset_session_client();
    app.login_session_no_content(&owner.email, &owner.password)
        .await;

    // 5 バイトのファイルを置いてから、テナントのクォータを 10 バイトに絞る。
    let uploaded = upload_file(
        &app,
        tp.tenant_id,
        None,
        "small.txt",
        "text/plain",
        b"hello",
    )
    .await;
    let file_id = file_id_of(&uploaded);
    set_tenant_quota(&app, tp.tenant_id, 10).await;

    // 旧サイズ(5) を差し引いても 10 を超える 20 バイトは拒否。
    let too_large = app
        .put_json_with_session(
            &content_path(tp.tenant_id, file_id),
            serde_json::json!({ "content": "12345678901234567890" }),
        )
        .await;
    assert_eq!(
        too_large.status(),
        StatusCode::PAYLOAD_TOO_LARGE,
        "クォータを超える差し替えは拒否される"
    );

    // 拒否されたときは中身もサイズも元のまま。
    let unchanged = app
        .get_with_session(&format!("/v1/drive/files/{file_id}/content"))
        .await;
    assert_eq!(
        unchanged.text().await.expect("content body"),
        "hello",
        "拒否時に元の内容が壊れていない"
    );

    // 対照: 旧サイズを差し引けば収まる 8 バイトは通る（過剰拒否でない）。
    let within = app
        .put_json_with_session(
            &content_path(tp.tenant_id, file_id),
            serde_json::json!({ "content": "12345678" }),
        )
        .await;
    assert_eq!(
        within.status(),
        StatusCode::OK,
        "旧サイズを差し引けば収まる差し替えは通る"
    );

    app.cleanup_user(owner.id).await;
}

/// 同一ファイルへの並行更新でも DB とストレージが整合し続けること。
///
/// ハンドラはテナント行と対象ファイル行をロックしてから旧サイズ・旧ストレージキーを
/// 読む。ロックが正しく効いていれば、2 本の更新は直列化され、最終状態はどちらか一方の
/// 内容と完全一致する（部分書き込みや DB と実体の食い違いが起きない）。500 も返さない。
///
/// 旧サイズをロック前に読んでいた頃は、並行更新が互いのサイズ差分を取り違えたり
/// 中間キーを二重削除して壊れ得た。確定的に競合を再現するのは難しいが、`tokio::join`
/// で 2 本同時に投げて「壊れない」ことを回帰として確認する。
#[tokio::test]
async fn update_content_concurrent_updates_stay_consistent() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;
    app.reset_session_client();
    app.login_session_no_content(&owner.email, &owner.password)
        .await;

    let uploaded = upload_file(
        &app,
        tp.tenant_id,
        None,
        "race.txt",
        "text/plain",
        b"initial",
    )
    .await;
    let file_id = file_id_of(&uploaded);

    let url = format!("{}{}", app.base_url(), content_path(tp.tenant_id, file_id));
    let body_a = "A".repeat(64);
    let body_b = "B".repeat(128);

    let client_a = app.session_client();
    let client_b = app.session_client();
    let (url_a, url_b) = (url.clone(), url.clone());
    let (send_a, send_b) = (body_a.clone(), body_b.clone());

    let (res_a, res_b) = tokio::join!(
        async move {
            client_a
                .put(&url_a)
                .json(&serde_json::json!({ "content": send_a }))
                .send()
                .await
        },
        async move {
            client_b
                .put(&url_b)
                .json(&serde_json::json!({ "content": send_b }))
                .send()
                .await
        },
    );

    let status_a = res_a.expect("request a").status();
    let status_b = res_b.expect("request b").status();
    for status in [status_a, status_b] {
        assert_ne!(
            status,
            StatusCode::INTERNAL_SERVER_ERROR,
            "並行更新でサーバーエラーを返さない"
        );
        assert!(
            status == StatusCode::OK || status == StatusCode::PAYLOAD_TOO_LARGE,
            "更新は成功かクォータ超過のいずれか (実際: {status})"
        );
    }

    // 最終状態はどちらか一方の内容と完全一致する（部分書き込みでない）。
    let content = app
        .get_with_session(&format!("/v1/drive/files/{file_id}/content"))
        .await;
    assert_eq!(content.status(), StatusCode::OK);
    let final_body = content.text().await.expect("content body");
    assert!(
        final_body == body_a || final_body == body_b,
        "最終内容はいずれかの更新と完全一致するべき (len={})",
        final_body.len()
    );

    // メタデータの size が実体のバイト数と一致する（DB とストレージが食い違わない）。
    let meta = app
        .get_with_session(&format!(
            "/v1/tenants/{}/drive/files/{file_id}",
            tp.tenant_id
        ))
        .await;
    assert_eq!(meta.status(), StatusCode::OK);
    let meta_body: serde_json::Value = meta.json().await.expect("meta json");
    assert_eq!(
        meta_body["size"].as_i64(),
        Some(final_body.len() as i64),
        "size が配信される内容のバイト数と一致する"
    );

    app.cleanup_user(owner.id).await;
}

/// テナントレベルファイル（project_id が NULL）の配信もテナント所属者に限定される。
///
/// 配信エンドポイントは URL に tenant_id を含まずファイル ID だけで引くため、
/// テナントレベルファイルを無条件に許可すると、ID を知る第三者が未認証で内容を取得できる。
#[tokio::test]
async fn tenant_level_file_content_requires_tenant_access() {
    let mut app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;
    app.reset_session_client();
    app.login_session_no_content(&owner.email, &owner.password)
        .await;

    // folder_id を指定しないアップロードは project_id が NULL のテナントレベルファイルになる。
    let uploaded = upload_file(
        &app,
        tp.tenant_id,
        None,
        "secret.txt",
        "text/plain",
        b"tenant secret",
    )
    .await;
    let file_id = file_id_of(&uploaded);
    let path = format!("/v1/drive/files/{file_id}/content");

    // 対照: アップロードしたテナントのオーナーは従来どおり読める。
    let allowed = app.get_with_session(&path).await;
    assert_eq!(
        allowed.status(),
        StatusCode::OK,
        "テナントのオーナーは読める"
    );
    assert_eq!(
        allowed.text().await.expect("content body"),
        "tenant secret",
        "内容が欠けずに取得できる"
    );

    // 未認証では読めない。
    app.reset_session_client();
    let anonymous = app.get(&path).await;
    assert_eq!(
        anonymous.status(),
        StatusCode::FORBIDDEN,
        "未認証では読めない"
    );

    // 別テナントのユーザーでも読めない。
    let outsider = app.insert_user(false, false).await;
    app.insert_tenant_project(outsider.id).await;
    app.reset_session_client();
    app.login_session_no_content(&outsider.email, &outsider.password)
        .await;
    let cross_tenant = app.get(&path).await;
    assert_eq!(
        cross_tenant.status(),
        StatusCode::FORBIDDEN,
        "別テナントのユーザーは読めない"
    );

    app.cleanup_user(outsider.id).await;
    app.cleanup_user(owner.id).await;
}
