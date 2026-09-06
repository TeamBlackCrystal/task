//! Drive folder CRUD, sharing, and public link handlers.

use crate::AppState;
use crate::error::AppError;
use crate::extractors::AuthUser;
use crate::openapi::{DriveFolderErrors, PublicShareErrors};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use axum_valid::Valid;
use chrono::Utc;
use entity::{
    drive_files, drive_folder_shares,
    drive_folder_shares::{SharePermission, validate_share_target_xor},
    drive_folders,
    scopes::Scope,
    users,
};
use payload::drive_files::DriveFileResponse;
use payload::drive_folder_shares::DriveFolderShareResponse;
use payload::drive_folders::*;
use rand::RngExt;
use sea_orm::prelude::Uuid;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait,
    QueryFilter, TransactionTrait,
};
use service::drive::{
    can_access_project, is_project_root_folder, is_tenant_owner, lock_tenant_drive,
    sync_subtree_project_id,
};

const SHARE_TOKEN_LEN: usize = 32;
const SHARE_TOKEN_CHARSET: &[u8] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

// --- Token generation ---

fn generate_share_token() -> String {
    let mut rng = rand::rng();
    (0..SHARE_TOKEN_LEN)
        .map(|_| {
            let idx = rng.random_range(0..SHARE_TOKEN_CHARSET.len());
            SHARE_TOKEN_CHARSET[idx] as char
        })
        .collect()
}

fn is_share_expired(expires_at: Option<&sea_orm::prelude::DateTimeWithTimeZone>) -> bool {
    expires_at
        .map(|t| t.with_timezone(&Utc) < Utc::now())
        .unwrap_or(false)
}

// --- DB helpers ---

async fn get_folder_in_tenant<C: ConnectionTrait>(
    conn: &C,
    tenant_id: Uuid,
    folder_id: Uuid,
) -> Result<drive_folders::Model, AppError> {
    drive_folders::Entity::find_by_id(folder_id)
        .filter(drive_folders::Column::TenantId.eq(tenant_id))
        .one(conn)
        .await?
        .ok_or(AppError::NotFound)
}

async fn require_folder_share_admin(
    state: &AppState,
    folder: &drive_folders::Model,
    user_id: Uuid,
) -> Result<(), AppError> {
    if folder.created_by == user_id {
        return Ok(());
    }
    if is_tenant_owner(&state.db, folder.tenant_id, user_id).await? {
        return Ok(());
    }
    Err(AppError::Forbidden)
}

/// プロジェクトフォルダの書き込み（作成・移動・削除）を許すか。
///
/// 一般フォルダ（`project_id` なし）はテナント所属で足りる。プロジェクトフォルダは
/// オーナーかそのプロジェクトに入れる人だけに許す（共有受信者は読み取り専用。
/// drive_files の upload / authorize_file_write と同方針）。
async fn require_folder_project_write<C: ConnectionTrait>(
    conn: &C,
    tenant_id: Uuid,
    project_id: Option<Uuid>,
    user_id: Uuid,
) -> Result<(), AppError> {
    let Some(project_id) = project_id else {
        return Ok(());
    };
    if is_tenant_owner(conn, tenant_id, user_id).await? {
        return Ok(());
    }
    if can_access_project(conn, tenant_id, project_id, user_id).await? {
        return Ok(());
    }
    Err(AppError::Forbidden)
}

async fn validate_parent_folder<C: ConnectionTrait>(
    conn: &C,
    tenant_id: Uuid,
    parent_id: Uuid,
    exclude_folder_id: Option<Uuid>,
) -> Result<(), AppError> {
    get_folder_in_tenant(conn, tenant_id, parent_id).await?;
    if let Some(folder_id) = exclude_folder_id {
        if parent_id == folder_id {
            return Err(AppError::BadRequest);
        }
        let mut current = Some(parent_id);
        while let Some(id) = current {
            if id == folder_id {
                return Err(AppError::BadRequest);
            }
            current = drive_folders::Entity::find_by_id(id)
                .one(conn)
                .await?
                .and_then(|f| f.parent_id);
        }
    }
    Ok(())
}

async fn folder_has_children<C: ConnectionTrait>(
    conn: &C,
    folder_id: Uuid,
) -> Result<bool, AppError> {
    let subfolder_count = drive_folders::Entity::find()
        .filter(drive_folders::Column::ParentId.eq(folder_id))
        .count(conn)
        .await?;
    if subfolder_count > 0 {
        return Ok(true);
    }
    let file_count = drive_files::Entity::find()
        .filter(drive_files::Column::FolderId.eq(folder_id))
        .count(conn)
        .await?;
    Ok(file_count > 0)
}

async fn get_share_in_folder(
    state: &AppState,
    folder_id: Uuid,
    share_id: Uuid,
) -> Result<drive_folder_shares::Model, AppError> {
    drive_folder_shares::Entity::find_by_id(share_id)
        .filter(drive_folder_shares::Column::FolderId.eq(folder_id))
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)
}

async fn load_active_share_by_token(
    state: &AppState,
    token: &str,
) -> Result<drive_folder_shares::Model, AppError> {
    let share = drive_folder_shares::Entity::find()
        .filter(drive_folder_shares::Column::ShareToken.eq(token))
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    if is_share_expired(share.expires_at.as_ref()) {
        return Err(AppError::Gone);
    }
    Ok(share)
}

async fn username_for_user(state: &AppState, user_id: Uuid) -> Result<String, AppError> {
    let user = users::Entity::find_by_id(user_id)
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(user.username)
}

async fn count_files_in_folder(state: &AppState, folder_id: Uuid) -> Result<u64, AppError> {
    drive_files::Entity::find()
        .filter(drive_files::Column::FolderId.eq(folder_id))
        .count(&state.db)
        .await
        .map_err(AppError::from)
}

fn parse_share_permission(permission: &str) -> Result<SharePermission, AppError> {
    match permission {
        "viewer" => Ok(SharePermission::Viewer),
        "editor" => Err(AppError::UnprocessableEntity),
        _ => Err(AppError::BadRequest),
    }
}

// --- Folder CRUD ---

#[utoipa::path(
    get,
    path = "/",
    tag = "Drive Folders",
    summary = "ドライブフォルダ一覧",
    params(("tenant_id" = Uuid, Path, description = "テナントID")),
    responses(
        (status = 200, description = "フォルダ一覧", body = [DriveFolderResponse]),
        DriveFolderErrors,
    ),
    security(("bearerAuth" = []))
)]
pub async fn list_folders(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(tenant_id): Path<Uuid>,
) -> Result<Json<Vec<DriveFolderResponse>>, AppError> {
    auth.require_scope(Scope::ReadDrive)?;
    auth.ensure_tenant_access(&state, tenant_id, None).await?;
    let folders = drive_folders::Entity::find()
        .filter(drive_folders::Column::TenantId.eq(tenant_id))
        .all(&state.db)
        .await?;
    Ok(Json(folders.into_iter().map(Into::into).collect()))
}

#[utoipa::path(
    post,
    path = "/",
    tag = "Drive Folders",
    summary = "ドライブフォルダ作成",
    params(("tenant_id" = Uuid, Path, description = "テナントID")),
    request_body = CreateFolderRequest,
    responses(
        (status = 201, description = "作成されたフォルダ", body = DriveFolderResponse),
        DriveFolderErrors,
    ),
    security(("bearerAuth" = []))
)]
pub async fn create_folder(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(tenant_id): Path<Uuid>,
    Valid(Json(payload)): Valid<Json<CreateFolderRequest>>,
) -> Result<(StatusCode, Json<DriveFolderResponse>), AppError> {
    auth.require_scope(Scope::WriteDrive)?;
    auth.ensure_tenant_access(&state, tenant_id, None).await?;
    // 親の project_id を継承する。継承しないと、プロジェクトフォルダの下に作った
    // フォルダとその中のファイルがテナント一般ファイル扱いになり、プロジェクトの
    // 非メンバーから見えてしまう（配信の認可は project_id で判定するため）。
    //
    // 親の読みと挿入は同一トランザクション、かつテナントの Drive をロックしてから行う。
    // 親が別プロジェクトへ移動している最中に読むと、移動前の project_id を継承した
    // 行が同期の後から挿入され、直したはずの漏れが再発する。
    let txn = state.db.begin().await?;
    lock_tenant_drive(&txn, tenant_id).await?;
    let parent_project_id = if let Some(parent_id) = payload.parent_id {
        validate_parent_folder(&txn, tenant_id, parent_id, None).await?;
        let parent = get_folder_in_tenant(&txn, tenant_id, parent_id).await?;
        require_folder_project_write(&txn, tenant_id, parent.project_id, auth.user_id).await?;
        parent.project_id
    } else {
        None
    };
    let folder = drive_folders::ActiveModel {
        id: Set(Uuid::new_v4()),
        name: Set(payload.name),
        parent_id: Set(payload.parent_id),
        tenant_id: Set(tenant_id),
        project_id: Set(parent_project_id),
        created_by: Set(auth.user_id),
        created_at: Set(Default::default()),
    };
    let model = folder.insert(&txn).await?;
    txn.commit().await?;
    Ok((StatusCode::CREATED, Json(model.into())))
}

#[utoipa::path(
    patch,
    path = "/{folder_id}",
    tag = "Drive Folders",
    summary = "ドライブフォルダ更新（名前変更・移動）",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("folder_id" = Uuid, Path, description = "フォルダID"),
    ),
    request_body = UpdateFolderRequest,
    responses(
        (status = 200, description = "更新されたフォルダ", body = DriveFolderResponse),
        DriveFolderErrors,
    ),
    security(("bearerAuth" = []))
)]
pub async fn update_folder(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, folder_id)): Path<(Uuid, Uuid)>,
    Json(payload): Json<UpdateFolderRequest>,
) -> Result<Json<DriveFolderResponse>, AppError> {
    auth.require_scope(Scope::WriteDrive)?;
    auth.ensure_tenant_access(&state, tenant_id, None).await?;
    if payload.name.is_none() && payload.parent_id.is_none() {
        // 変更が無いなら読むだけ。ロックを取る必要も無い
        let folder = get_folder_in_tenant(&state.db, tenant_id, folder_id).await?;
        return Ok(Json(folder.into()));
    }
    if let Some(name) = &payload.name
        && name.is_empty()
    {
        return Err(AppError::BadRequest);
    }

    // 読みも ACL 検査も書き込みと同じトランザクションに入れ、テナントの Drive を
    // ロックしてから始める。ロックの外で読むと、移動元・移動先の project_id が
    // 読んだ後に変わったり、配下の同期を集めた後に子が挿入されたりして、
    // 直したはずの ACL 漏れが再発する
    let txn = state.db.begin().await?;
    lock_tenant_drive(&txn, tenant_id).await?;

    let folder = get_folder_in_tenant(&txn, tenant_id, folder_id).await?;

    // 移動元の ACL。プロジェクトフォルダを動かせるのはそのプロジェクトに入れる人だけ
    require_folder_project_write(&txn, tenant_id, folder.project_id, auth.user_id).await?;

    let is_move = payload
        .parent_id
        .is_some_and(|parent_id| parent_id != folder.parent_id);

    // 自動生成のプロジェクトルートフォルダは動かさない。動かすとプロジェクトと
    // ドライブの 1 対 1 が壊れ、配下の project_id を戻す手立ても無くなる
    if is_move && is_project_root_folder(&folder) {
        return Err(AppError::Conflict);
    }

    // 移動先の ACL と、移動によって配下が属することになる project_id
    let destination_project_id = if let Some(Some(pid)) = payload.parent_id {
        validate_parent_folder(&txn, tenant_id, pid, Some(folder_id)).await?;
        let parent = get_folder_in_tenant(&txn, tenant_id, pid).await?;
        require_folder_project_write(&txn, tenant_id, parent.project_id, auth.user_id).await?;
        parent.project_id
    } else {
        None
    };

    let previous_project_id = folder.project_id;
    let mut active: drive_folders::ActiveModel = folder.into();
    if let Some(name) = payload.name {
        active.name = Set(name);
    }
    if let Some(parent_id) = payload.parent_id {
        active.parent_id = Set(parent_id);
    }

    if !is_move {
        let model = active.update(&txn).await?;
        txn.commit().await?;
        return Ok(Json(model.into()));
    }

    // 移動と配下の project_id 同期は同一トランザクションで行う。片方だけ通ると
    // 「プロジェクトファイルなのに別プロジェクトの配下」という状態が残る
    active.project_id = Set(destination_project_id);
    let model = active.update(&txn).await?;
    if destination_project_id != previous_project_id {
        sync_subtree_project_id(&txn, model.id, destination_project_id).await?;
    }
    // 同期後の値で返す（sync_subtree_project_id は update_many なので model に載らない）
    let refreshed = get_folder_in_tenant(&txn, tenant_id, model.id).await?;
    txn.commit().await?;
    Ok(Json(refreshed.into()))
}

#[utoipa::path(
    delete,
    path = "/{folder_id}",
    tag = "Drive Folders",
    summary = "ドライブフォルダ削除",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("folder_id" = Uuid, Path, description = "フォルダID"),
    ),
    responses(
        (status = 204, description = "削除成功"),
        DriveFolderErrors,
    ),
    security(("bearerAuth" = []))
)]
pub async fn delete_folder(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, folder_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    auth.require_scope(Scope::WriteDrive)?;
    auth.ensure_tenant_access(&state, tenant_id, None).await?;
    // 子の有無を読んでから消すまでの間に、その子として新しいフォルダを挿入されうる。
    // drive_folders.parent_id は ON DELETE SET NULL なので、割り込まれた子は
    // parent_id = NULL かつ project_id = Some のままドライブ直下へ出る。これは
    // 自動生成のプロジェクトルートと同じ形で、以後は更新も削除も 409 で拒まれ、
    // API から片付けられなくなる（drive_files と違い、この表には受け止める CHECK が無い）。
    // 作成・移動と同じテナントロックの下で、確認から削除までを一続きにする。
    let txn = state.db.begin().await?;
    lock_tenant_drive(&txn, tenant_id).await?;
    let folder = get_folder_in_tenant(&txn, tenant_id, folder_id).await?;
    require_folder_project_write(&txn, tenant_id, folder.project_id, auth.user_id).await?;
    // プロジェクトルートフォルダは空でも消さない。破棄はプロジェクト削除の CASCADE に任せる
    if is_project_root_folder(&folder) {
        return Err(AppError::Conflict);
    }
    if folder_has_children(&txn, folder_id).await? {
        return Err(AppError::Conflict);
    }
    drive_folders::Entity::delete_by_id(folder.id)
        .exec(&txn)
        .await?;
    txn.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

// --- Share APIs (authenticated) ---

#[utoipa::path(
    get,
    path = "/{folder_id}/shares",
    tag = "Drive Shares",
    summary = "フォルダ共有一覧",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("folder_id" = Uuid, Path, description = "フォルダID"),
    ),
    responses(
        (status = 200, description = "共有一覧", body = [DriveFolderShareResponse]),
        DriveFolderErrors,
    ),
    security(("bearerAuth" = []))
)]
pub async fn list_shares(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, folder_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<DriveFolderShareResponse>>, AppError> {
    auth.require_scope(Scope::ReadDrive)?;
    auth.ensure_tenant_access(&state, tenant_id, None).await?;
    let folder = get_folder_in_tenant(&state.db, tenant_id, folder_id).await?;
    require_folder_share_admin(&state, &folder, auth.user_id).await?;
    let shares = drive_folder_shares::Entity::find()
        .filter(drive_folder_shares::Column::FolderId.eq(folder_id))
        .all(&state.db)
        .await?;
    Ok(Json(shares.into_iter().map(Into::into).collect()))
}

#[utoipa::path(
    post,
    path = "/{folder_id}/shares",
    tag = "Drive Shares",
    summary = "フォルダ共有作成",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("folder_id" = Uuid, Path, description = "フォルダID"),
    ),
    request_body = CreateShareRequest,
    responses(
        (status = 201, description = "作成された共有", body = DriveFolderShareResponse),
        DriveFolderErrors,
    ),
    security(("bearerAuth" = []))
)]
pub async fn create_share(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, folder_id)): Path<(Uuid, Uuid)>,
    Json(payload): Json<CreateShareRequest>,
) -> Result<(StatusCode, Json<DriveFolderShareResponse>), AppError> {
    auth.require_scope(Scope::WriteDrive)?;
    auth.ensure_tenant_access(&state, tenant_id, None).await?;
    let folder = get_folder_in_tenant(&state.db, tenant_id, folder_id).await?;
    require_folder_share_admin(&state, &folder, auth.user_id).await?;

    let permission = parse_share_permission(&payload.permission)?;

    let (shared_with_user_id, share_token) = match payload.share_type.as_str() {
        "user" => {
            let user_id = payload.user_id.ok_or(AppError::BadRequest)?;
            validate_share_target_xor(Some(user_id), None)?;
            (Some(user_id), None)
        }
        "public_link" => {
            let token = generate_share_token();
            validate_share_target_xor(None, Some(&token))?;
            (None, Some(token))
        }
        _ => return Err(AppError::BadRequest),
    };

    let share = drive_folder_shares::ActiveModel {
        id: Set(Uuid::new_v4()),
        folder_id: Set(folder_id),
        shared_with_user_id: Set(shared_with_user_id),
        share_token: Set(share_token),
        permission: Set(permission),
        created_by: Set(auth.user_id),
        expires_at: Set(payload.expires_at),
        created_at: Set(Default::default()),
    };
    let model = share.insert(&state.db).await?;
    Ok((StatusCode::CREATED, Json(model.into())))
}

#[utoipa::path(
    delete,
    path = "/{folder_id}/shares/{share_id}",
    tag = "Drive Shares",
    summary = "フォルダ共有取り消し",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("folder_id" = Uuid, Path, description = "フォルダID"),
        ("share_id" = Uuid, Path, description = "共有ID"),
    ),
    responses(
        (status = 204, description = "削除成功"),
        DriveFolderErrors,
    ),
    security(("bearerAuth" = []))
)]
pub async fn delete_share(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, folder_id, share_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    auth.require_scope(Scope::WriteDrive)?;
    auth.ensure_tenant_access(&state, tenant_id, None).await?;
    let folder = get_folder_in_tenant(&state.db, tenant_id, folder_id).await?;
    require_folder_share_admin(&state, &folder, auth.user_id).await?;
    let share = get_share_in_folder(&state, folder_id, share_id).await?;
    drive_folder_shares::Entity::delete_by_id(share.id)
        .exec(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// --- Public link APIs (no auth) ---
//
// path は routes 側の `.nest("/drive", ...)` 位置からの相対で書く。ここに
// `/v1/drive/...` と絶対で書くと二重連結され、`/v1/drive/v1/drive/share/{token}`
// として登録される（OpenAPI も同じ URL になり、仕様の口では 404 になる）。
// 同じ nest に載る drive_files::get_file_content が `/files/{id}/content` と
// 相対で書いているのが正しい形。

#[utoipa::path(
    get,
    path = "/share/{token}",
    tag = "Drive Shares",
    summary = "公開リンクでフォルダメタデータ取得（認証不要）",
    params(("token" = String, Path, description = "共有トークン")),
    responses(
        (status = 200, description = "フォルダメタデータ", body = PublicFolderResponse),
        PublicShareErrors,
    )
)]
pub async fn get_public_share_folder(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Json<PublicFolderResponse>, AppError> {
    let share = load_active_share_by_token(&state, &token).await?;
    let folder = drive_folders::Entity::find_by_id(share.folder_id)
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    let created_by_name = username_for_user(&state, folder.created_by).await?;
    let file_count = count_files_in_folder(&state, folder.id).await?;
    Ok(Json(PublicFolderResponse {
        name: folder.name,
        created_by_name,
        file_count,
    }))
}

#[utoipa::path(
    get,
    path = "/share/{token}/files",
    tag = "Drive Shares",
    summary = "公開リンク経由でファイル一覧取得（認証不要）",
    params(("token" = String, Path, description = "共有トークン")),
    responses(
        (status = 200, description = "ファイル一覧", body = [DriveFileResponse]),
        PublicShareErrors,
    )
)]
pub async fn list_public_share_files(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Json<Vec<DriveFileResponse>>, AppError> {
    let share = load_active_share_by_token(&state, &token).await?;
    let files = drive_files::Entity::find()
        .filter(drive_files::Column::FolderId.eq(share.folder_id))
        .all(&state.db)
        .await?;
    Ok(Json(files.into_iter().map(Into::into).collect()))
}
