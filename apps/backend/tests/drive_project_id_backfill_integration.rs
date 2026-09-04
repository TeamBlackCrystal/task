mod common;

use common::TestApp;
use entity::{drive_files, drive_folders, projects};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ConnectionTrait, EntityTrait};
use uuid::Uuid;

// 既に project_id が食い違っている行を戻す backfill の検査。
//
// 作成・移動の側は直したが、修正前に作られた行は project_id = NULL のまま残る。
// プロジェクトフォルダ配下の子フォルダとその中のファイルが「一般ファイル」として
// 非メンバーに見えたままになるので、マイグレーションで揃える。
//
// SQL は `apps/backend/sql/backfill_drive_project_ids.sql` に置き、マイグレーション
// （`m20260904000000_drive_project_id_backfill`）とここで同じものを使う。migration crate は
// ワークスペースから exclude されていて関数を呼べないため、ファイルを共有する。
// テストハーネスはエンティティからスキーマを組む（マイグレーションを流さない）ので、
// ここでは食い違った状態を作ってから SQL 本体を当てる。
const BACKFILL_SQL: &str = include_str!("../sql/backfill_drive_project_ids.sql");

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

async fn insert_folder(
    app: &TestApp,
    tenant_id: Uuid,
    created_by: Uuid,
    parent_id: Option<Uuid>,
    project_id: Option<Uuid>,
) -> Uuid {
    let folder_id = Uuid::new_v4();
    drive_folders::ActiveModel {
        id: Set(folder_id),
        name: Set("folder".into()),
        parent_id: Set(parent_id),
        tenant_id: Set(tenant_id),
        project_id: Set(project_id),
        created_by: Set(created_by),
        created_at: Set(chrono::Utc::now().into()),
    }
    .insert(&app.state.db)
    .await
    .expect("insert folder");
    folder_id
}

/// 修正前の `upload_file` と同じで、フォルダの `project_id` をそのまま入れる。
async fn insert_file(
    app: &TestApp,
    tenant_id: Uuid,
    folder_id: Option<Uuid>,
    uploader: Uuid,
) -> Uuid {
    let project_id = match folder_id {
        Some(id) => {
            drive_folders::Entity::find_by_id(id)
                .one(&app.state.db)
                .await
                .expect("load folder")
                .expect("folder exists")
                .project_id
        }
        None => None,
    };
    let now = chrono::Utc::now();
    let file_id = Uuid::new_v4();
    drive_files::ActiveModel {
        id: Set(file_id),
        name: Set("note.txt".into()),
        size: Set(5),
        mime_type: Set("text/plain".into()),
        storage_type: Set(drive_files::StorageType::Local),
        storage_key: Set(Uuid::new_v4().to_string()),
        tenant_id: Set(tenant_id),
        project_id: Set(project_id),
        uploader_id: Set(uploader),
        folder_id: Set(folder_id),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
    }
    .insert(&app.state.db)
    .await
    .expect("insert file");
    file_id
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

async fn run_backfill(app: &TestApp) {
    app.state
        .db
        .execute_unprepared(BACKFILL_SQL)
        .await
        .expect("run backfill");
}

/// プロジェクトルート配下は、何段下でもルートの `project_id` に揃う。
#[tokio::test]
async fn backfill_propagates_the_project_down_the_whole_subtree() {
    let app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;

    // プロジェクトルート（自動生成と同じ形）
    let root = insert_folder(&app, tp.tenant_id, owner.id, None, Some(tp.project_id)).await;
    let root_file = insert_file(&app, tp.tenant_id, Some(root), owner.id).await;

    // 修正前の create_folder は継承しないので、配下は NULL のまま。
    // 2 段では「1 段だけ辿る実装」を隠すので 4 段にする
    let mut chain = Vec::new();
    let mut parent = root;
    for _ in 0..4 {
        parent = insert_folder(&app, tp.tenant_id, owner.id, Some(parent), None).await;
        chain.push(parent);
    }
    let mut files = Vec::new();
    for folder in &chain {
        files.push(insert_file(&app, tp.tenant_id, Some(*folder), owner.id).await);
    }

    // 前提: backfill 前は配下が一般ファイル扱い
    for folder in &chain {
        assert_eq!(folder_project_id(&app, *folder).await, None);
    }
    for file in &files {
        assert_eq!(file_project_id(&app, *file).await, None);
    }

    run_backfill(&app).await;

    assert_eq!(
        folder_project_id(&app, root).await,
        Some(tp.project_id),
        "ルート自身は変わらない"
    );
    assert_eq!(file_project_id(&app, root_file).await, Some(tp.project_id));
    for (depth, folder) in chain.iter().enumerate() {
        assert_eq!(
            folder_project_id(&app, *folder).await,
            Some(tp.project_id),
            "{}段目のフォルダが揃っていない",
            depth + 1
        );
    }
    for (depth, file) in files.iter().enumerate() {
        assert_eq!(
            file_project_id(&app, *file).await,
            Some(tp.project_id),
            "{}段目のファイルが揃っていない",
            depth + 1
        );
    }
}

/// プロジェクトの外は触らない。
///
/// 一般ツリーの配下に `project_id` を持つ行は、階層より厳しい判定になるだけで
/// 漏れる向きではない。NULL へ落とすと非メンバーへ開いてしまうので backfill の対象外。
#[tokio::test]
async fn backfill_leaves_rows_outside_project_roots_alone() {
    let app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;
    let other_project = insert_extra_project(&app, tp.tenant_id).await;

    // 一般ツリー（親なし・project_id なし）
    let plain_root = insert_folder(&app, tp.tenant_id, owner.id, None, None).await;
    let plain_child = insert_folder(&app, tp.tenant_id, owner.id, Some(plain_root), None).await;
    let plain_file = insert_file(&app, tp.tenant_id, Some(plain_child), owner.id).await;

    // 一般ツリーの配下だが project_id を持つ行（移動の取りこぼしで残りうる形）
    let stricter = insert_folder(
        &app,
        tp.tenant_id,
        owner.id,
        Some(plain_root),
        Some(other_project),
    )
    .await;
    let stricter_file = insert_file(&app, tp.tenant_id, Some(stricter), owner.id).await;

    // フォルダに属さないファイル（ドライブ直下）は階層から project_id を導けない
    let loose_file = insert_file(&app, tp.tenant_id, None, owner.id).await;

    run_backfill(&app).await;

    assert_eq!(folder_project_id(&app, plain_root).await, None);
    assert_eq!(folder_project_id(&app, plain_child).await, None);
    assert_eq!(file_project_id(&app, plain_file).await, None);
    assert_eq!(
        folder_project_id(&app, stricter).await,
        Some(other_project),
        "厳しい側の project_id を NULL へ落とさない"
    );
    assert_eq!(
        file_project_id(&app, stricter_file).await,
        Some(other_project)
    );
    assert_eq!(file_project_id(&app, loose_file).await, None);
}

/// 別プロジェクトのツリーが混ざっても、それぞれのルートの値になる。
#[tokio::test]
async fn backfill_keeps_projects_separate() {
    let app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;
    let other_project = insert_extra_project(&app, tp.tenant_id).await;

    let root_a = insert_folder(&app, tp.tenant_id, owner.id, None, Some(tp.project_id)).await;
    let child_a = insert_folder(&app, tp.tenant_id, owner.id, Some(root_a), None).await;
    let file_a = insert_file(&app, tp.tenant_id, Some(child_a), owner.id).await;

    let root_b = insert_folder(&app, tp.tenant_id, owner.id, None, Some(other_project)).await;
    let child_b = insert_folder(&app, tp.tenant_id, owner.id, Some(root_b), None).await;
    let file_b = insert_file(&app, tp.tenant_id, Some(child_b), owner.id).await;

    run_backfill(&app).await;

    assert_eq!(folder_project_id(&app, child_a).await, Some(tp.project_id));
    assert_eq!(file_project_id(&app, file_a).await, Some(tp.project_id));
    assert_eq!(folder_project_id(&app, child_b).await, Some(other_project));
    assert_eq!(file_project_id(&app, file_b).await, Some(other_project));
}

/// 別プロジェクトのルートが入れ子になっている既存データは、書き換えずに失敗させる。
///
/// 修正前はプロジェクトルートの移動も移動先 ACL の無視もできたので、
/// 「A のルートが B のツリー配下にある」状態を作れた。親の値をそのまま伝播すると
/// A のルートと配下のファイルが B のものになり、A のファイルが B のメンバーへ公開され、
/// A のメンバーはアクセスを失う。backfill が新しい漏れを作ってはいけない。
#[tokio::test]
async fn backfill_refuses_to_absorb_a_nested_foreign_project_root() {
    let app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;
    let other_project = insert_extra_project(&app, tp.tenant_id).await;

    // B のツリー
    let root_b = insert_folder(&app, tp.tenant_id, owner.id, None, Some(other_project)).await;
    // 修正前に B の配下へ移された A のルート（parent_id が付いた時点でルート判定から外れる）
    let nested_a = insert_folder(
        &app,
        tp.tenant_id,
        owner.id,
        Some(root_b),
        Some(tp.project_id),
    )
    .await;
    let file_a = insert_file(&app, tp.tenant_id, Some(nested_a), owner.id).await;

    let result = app.state.db.execute_unprepared(BACKFILL_SQL).await;

    assert!(
        result.is_err(),
        "境界が食い違う既存データは、書き換えずに失敗させる"
    );
    assert_eq!(
        folder_project_id(&app, nested_a).await,
        Some(tp.project_id),
        "A のフォルダを B のものにしない"
    );
    assert_eq!(
        file_project_id(&app, file_a).await,
        Some(tp.project_id),
        "A のファイルを B のものにしない"
    );
}

/// 2 回流しても結果が変わらない（デプロイのたびに適用されても壊れない）。
#[tokio::test]
async fn backfill_is_idempotent() {
    let app = TestApp::new().await;

    let owner = app.insert_user(false, false).await;
    let tp = app.insert_tenant_project(owner.id).await;

    let root = insert_folder(&app, tp.tenant_id, owner.id, None, Some(tp.project_id)).await;
    let child = insert_folder(&app, tp.tenant_id, owner.id, Some(root), None).await;
    let file = insert_file(&app, tp.tenant_id, Some(child), owner.id).await;

    run_backfill(&app).await;
    let after_first = (
        folder_project_id(&app, child).await,
        file_project_id(&app, file).await,
    );
    run_backfill(&app).await;

    assert_eq!(after_first, (Some(tp.project_id), Some(tp.project_id)));
    assert_eq!(
        after_first,
        (
            folder_project_id(&app, child).await,
            file_project_id(&app, file).await
        )
    );
}
