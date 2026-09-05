use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::{
    Json,
    body::Body,
    extract::{Multipart, Path, Query, State},
    http::{StatusCode, header},
    response::Response,
};
use axum_valid::Valid;
use bytes::Bytes;
use chrono::Utc;
use futures::{SinkExt, channel::mpsc as fmpsc};
use sea_orm::prelude::Uuid;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, TransactionTrait,
};

use crate::AppState;
use crate::error::{AppError, ServerError};
use crate::extractors::{AuthUser, OptionalAuthUser};
use crate::openapi::CrudErrors;
use entity::{drive_files, drive_folder_shares, drive_folders, scopes::Scope, tenants};
use payload::drive_files::*;
use service::drive::{
    can_access_project, current_storage_type, effective_quota, guess_mime, is_editable_mime,
    is_tenant_owner, lock_tenant_drive, source_mime_override, tenant_used_bytes,
};
use service::storage::{ByteStream, StorageError};

const MAX_LIST_LIMIT: u32 = 200;

async fn load_tenant_file(
    state: &AppState,
    tenant_id: Uuid,
    file_id: Uuid,
) -> Result<drive_files::Model, AppError> {
    load_tenant_file_conn(&state.db, tenant_id, file_id).await
}

/// トランザクション内から読むための版。ロックを取ったあとに読み直す経路で使う。
async fn load_tenant_file_conn<C: ConnectionTrait>(
    conn: &C,
    tenant_id: Uuid,
    file_id: Uuid,
) -> Result<drive_files::Model, AppError> {
    drive_files::Entity::find_by_id(file_id)
        .filter(drive_files::Column::TenantId.eq(tenant_id))
        .one(conn)
        .await?
        .ok_or(AppError::NotFound)
}

async fn load_folder_in_tenant(
    state: &AppState,
    tenant_id: Uuid,
    folder_id: Uuid,
) -> Result<drive_folders::Model, AppError> {
    load_folder_in_tenant_conn(&state.db, tenant_id, folder_id).await
}

/// トランザクション内から読むための版。ロックを取ったあとに読み直す経路で使う。
async fn load_folder_in_tenant_conn<C: ConnectionTrait>(
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

async fn folder_has_user_share(
    state: &AppState,
    folder_id: Uuid,
    user_id: Uuid,
) -> Result<bool, AppError> {
    let mut current = Some(folder_id);
    while let Some(fid) = current {
        let share = drive_folder_shares::Entity::find()
            .filter(drive_folder_shares::Column::FolderId.eq(fid))
            .filter(drive_folder_shares::Column::SharedWithUserId.eq(user_id))
            .one(&state.db)
            .await?;
        if let Some(share) = share
            && share.expires_at.map(|e| e > Utc::now()).unwrap_or(true)
        {
            return Ok(true);
        }
        let folder = drive_folders::Entity::find_by_id(fid)
            .one(&state.db)
            .await?;
        current = folder.and_then(|f| f.parent_id);
    }
    Ok(false)
}

async fn folder_has_token_share(
    state: &AppState,
    folder_id: Uuid,
    token: &str,
) -> Result<bool, AppError> {
    let mut current = Some(folder_id);
    while let Some(fid) = current {
        let share = drive_folder_shares::Entity::find()
            .filter(drive_folder_shares::Column::FolderId.eq(fid))
            .filter(drive_folder_shares::Column::ShareToken.eq(token))
            .one(&state.db)
            .await?;
        if let Some(share) = share {
            if share.expires_at.map(|e| e > Utc::now()).unwrap_or(true) {
                return Ok(true);
            }
            return Ok(false);
        }
        let folder = drive_folders::Entity::find_by_id(fid)
            .one(&state.db)
            .await?;
        current = folder.and_then(|f| f.parent_id);
    }
    Ok(false)
}

async fn can_access_file_content(
    state: &AppState,
    file: &drive_files::Model,
    auth: &OptionalAuthUser,
    share_token: Option<&str>,
) -> Result<(), AppError> {
    // 共有トークンはフォルダ単位で判定する。テナントレベルファイルも同じ経路に載せる。
    if let Some(token) = share_token.filter(|t| !t.is_empty()) {
        if let Some(folder_id) = file.folder_id
            && folder_has_token_share(state, folder_id, token).await?
        {
            return Ok(());
        }
        return Err(AppError::Forbidden);
    }

    let Some(auth_user) = &auth.0 else {
        return Err(AppError::Forbidden);
    };
    auth_user.require_scope(Scope::ReadDrive)?;

    // この経路は URL に tenant_id を含まず、ファイル ID だけで引く。テナントレベルファイル
    // （project_id が NULL）を無条件に許可すると、ID を知る第三者に内容が渡るため、
    // ファイル自身の tenant_id に対してテナント境界を確認する。
    let Some(project_id) = file.project_id else {
        return auth_user
            .ensure_tenant_access(state, file.tenant_id, None)
            .await;
    };

    if is_tenant_owner(&state.db, file.tenant_id, auth_user.user_id).await? {
        return Ok(());
    }

    if can_access_project(&state.db, file.tenant_id, project_id, auth_user.user_id).await? {
        return Ok(());
    }

    if let Some(folder_id) = file.folder_id
        && folder_has_user_share(state, folder_id, auth_user.user_id).await?
    {
        return Ok(());
    }

    Err(AppError::Forbidden)
}

/// プロジェクトファイルのメタデータ読み取り権限を確認する。
/// project_id が NULL（テナントレベルファイル）は常に許可。
/// プロジェクトファイルはテナントオーナー、プロジェクトメンバー、フォルダ共有受信者のみ許可。
async fn authorize_file_access(
    state: &AppState,
    file: &drive_files::Model,
    user: &AuthUser,
) -> Result<(), AppError> {
    let Some(project_id) = file.project_id else {
        return Ok(());
    };
    if is_tenant_owner(&state.db, file.tenant_id, user.user_id).await? {
        return Ok(());
    }
    if can_access_project(&state.db, file.tenant_id, project_id, user.user_id).await? {
        return Ok(());
    }
    if let Some(folder_id) = file.folder_id
        && folder_has_user_share(state, folder_id, user.user_id).await?
    {
        return Ok(());
    }
    Err(AppError::Forbidden)
}

/// プロジェクトファイルの書き込み権限を確認する（共有受信者は読み取り専用のため除外）。
///
/// ロックを取ったトランザクションからも呼べるように接続を受け取る。移動を伴う経路は
/// ロックの内側で読み直したファイルを渡すこと（外で認可すると、ロック待ちのあいだの
/// 移動を見落とす）。
async fn authorize_file_write<C: ConnectionTrait>(
    conn: &C,
    file: &drive_files::Model,
    user: &AuthUser,
) -> Result<(), AppError> {
    let Some(project_id) = file.project_id else {
        return Ok(());
    };
    if is_tenant_owner(conn, file.tenant_id, user.user_id).await? {
        return Ok(());
    }
    if can_access_project(conn, file.tenant_id, project_id, user.user_id).await? {
        return Ok(());
    }
    Err(AppError::Forbidden)
}

fn list_limit(limit: u32) -> u32 {
    limit.clamp(1, MAX_LIST_LIMIT)
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/files",
    tag = "Drive Files",
    summary = "ドライブファイル一覧",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ListFilesQuery,
    ),
    responses(
        (status = 200, description = "ファイル一覧", body = ListFilesResponse),
        CrudErrors,
    )
)]
pub async fn list_files(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(tenant_id): Path<Uuid>,
    Query(query): Query<ListFilesQuery>,
) -> Result<Json<ListFilesResponse>, AppError> {
    auth.require_scope(Scope::ReadDrive)?;
    auth.ensure_tenant_access(&state, tenant_id, None).await?;

    let limit = list_limit(query.limit);

    // フォルダ指定時: そのフォルダがプロジェクト配下なら ACL を確認する
    if let Some(folder_id) = query.folder_id {
        let folder = load_folder_in_tenant(&state, tenant_id, folder_id).await?;
        if let Some(project_id) = folder.project_id
            && !is_tenant_owner(&state.db, tenant_id, auth.user_id).await?
            && !can_access_project(&state.db, tenant_id, project_id, auth.user_id).await?
            && !folder_has_user_share(&state, folder_id, auth.user_id).await?
        {
            return Err(AppError::Forbidden);
        }
    }

    let mut selector = drive_files::Entity::find()
        .filter(drive_files::Column::TenantId.eq(tenant_id))
        .order_by_desc(drive_files::Column::CreatedAt);

    selector = match query.folder_id {
        Some(folder_id) => selector.filter(drive_files::Column::FolderId.eq(folder_id)),
        None => selector.filter(drive_files::Column::FolderId.is_null()),
    };

    let total = selector.clone().count(&state.db).await?;
    let files = selector
        .offset(query.offset as u64)
        .limit(limit as u64)
        .all(&state.db)
        .await?;

    Ok(Json(ListFilesResponse {
        files: files.into_iter().map(Into::into).collect(),
        total,
    }))
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/files",
    tag = "Drive Files",
    summary = "ドライブファイルアップロード",
    params(("tenant_id" = Uuid, Path, description = "テナントID")),
    request_body(content_type = "multipart/form-data"),
    responses(
        (status = 201, description = "作成されたファイル", body = DriveFileResponse),
        CrudErrors,
    )
)]
pub async fn upload_file(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(tenant_id): Path<Uuid>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<DriveFileResponse>), AppError> {
    auth.require_scope(Scope::WriteDrive)?;
    auth.ensure_tenant_access(&state, tenant_id, None).await?;

    let tenant = tenants::Entity::find_by_id(tenant_id)
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    // 'name' と 'folder_id' は 'file' フィールドより前に送ること（ストリーミング設計の前提）。
    let mut display_name: Option<String> = None;
    let mut folder_id: Option<Uuid> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::Internal(e.into()))?
    {
        match field.name() {
            Some("file") => {
                let original_filename = field.file_name().map(str::to_string);
                let content_type = field.content_type().map(str::to_string);

                let name = display_name
                    .or_else(|| original_filename.clone())
                    .ok_or(AppError::BadRequest)?;
                // ソースコード拡張子はクライアント申告より上書き表を優先する。
                // ブラウザは .ts を video/mp2t のように申告することがあり、
                // それを保存すると本文編集エンドポイントが弾いてしまう。
                let mime_source = original_filename.as_deref().unwrap_or(name.as_str());
                let mime_type = source_mime_override(mime_source)
                    .map(str::to_string)
                    .or(content_type)
                    .unwrap_or_else(|| guess_mime(mime_source));

                // クォータ事前チェック（既に上限に達していれば即拒否）
                let used = tenant_used_bytes(&state.db, tenant_id).await?;
                if let Some(q) = effective_quota(&tenant, &state.drive_config)
                    && used >= q
                {
                    return Err(AppError::ContentTooLarge);
                }

                // 本文を流す前に落とすための先読み。確定値は下の txn 内で取り直す
                // （アップロード中にフォルダが移動すると、ここで読んだ project_id は古い）
                if let Some(fid) = folder_id {
                    let folder = load_folder_in_tenant(&state, tenant_id, fid).await?;
                    // プロジェクトフォルダへの書き込みはオーナーかメンバーのみ
                    // （共有受信者は読み取り専用。authorize_file_write と同方針）。
                    if let Some(project_id) = folder.project_id
                        && !is_tenant_owner(&state.db, tenant_id, auth.user_id).await?
                        && !can_access_project(&state.db, tenant_id, project_id, auth.user_id)
                            .await?
                    {
                        return Err(AppError::Forbidden);
                    }
                }

                // Field<'a> は 'static でないため stream::unfold 不可。
                // mpsc channel の Receiver は 'static なので ByteStream として使える。
                // tokio::join! で pump と upload を同タスク内で並行実行する。
                let byte_count = Arc::new(AtomicU64::new(0));
                let counter = byte_count.clone();
                let max_bytes = state.drive_config.upload_max_bytes;

                let (mut tx, rx) = fmpsc::channel::<Result<Bytes, StorageError>>(8);
                let byte_stream: ByteStream = Box::pin(rx);

                let pump = async move {
                    let mut field = field;
                    loop {
                        match field.chunk().await {
                            Ok(Some(bytes)) => {
                                let new_count = counter
                                    .fetch_add(bytes.len() as u64, Ordering::Relaxed)
                                    + bytes.len() as u64;
                                if new_count > max_bytes {
                                    // 上限超過: エラーを送ってストリームを早期終了
                                    let _ = tx
                                        .send(Err(StorageError::Other(
                                            "upload size limit exceeded".into(),
                                        )))
                                        .await;
                                    return;
                                }
                                if tx.send(Ok(bytes)).await.is_err() {
                                    return;
                                }
                            }
                            Ok(None) => break,
                            Err(e) => {
                                let _ = tx.send(Err(StorageError::Other(e.to_string()))).await;
                                return;
                            }
                        }
                    }
                };

                let file_id = Uuid::new_v4();
                let storage_key = file_id.to_string();

                let (upload_result, _) = tokio::join!(
                    state
                        .storage
                        .upload(&storage_key, byte_stream, 0, &mime_type),
                    pump
                );

                let actual_size = byte_count.load(Ordering::Relaxed);

                // サイズチェックを upload_result より先に行う。
                // 早期中断時は upload_result がエラーになるが、
                // actual_size > max_bytes の判定で正確な 413 を返す。
                if actual_size == 0 {
                    let _ = state.storage.delete(&storage_key).await;
                    return Err(AppError::BadRequest);
                }
                if actual_size > max_bytes {
                    let _ = state.storage.delete(&storage_key).await;
                    return Err(AppError::ContentTooLarge);
                }

                upload_result.map_err(storage_to_app_error)?;
                // テナント行を FOR UPDATE でロックしてクォータ確定チェックと INSERT をアトミックに実行。
                // 並行アップロードが同時にクォータチェックを通過してしまう競合を防ぐ。
                let now = Utc::now();
                let result: Result<drive_files::Model, AppError> = async {
                    let txn = state.db.begin().await?;
                    // 階層の読みと挿入を直列化する。ロックの外で読むと、アップロード中に
                    // フォルダが別プロジェクトへ移動していても古い project_id で入り、
                    // 配下の同期が終わった後の行として残ってしまう
                    lock_tenant_drive(&txn, tenant_id).await?;
                    let folder_project_id = if let Some(fid) = folder_id {
                        let folder = load_folder_in_tenant_conn(&txn, tenant_id, fid).await?;
                        if let Some(project_id) = folder.project_id
                            && !is_tenant_owner(&txn, tenant_id, auth.user_id).await?
                            && !can_access_project(&txn, tenant_id, project_id, auth.user_id)
                                .await?
                        {
                            return Err(AppError::Forbidden);
                        }
                        folder.project_id
                    } else {
                        None
                    };
                    let tenant_q = tenants::Entity::find_by_id(tenant_id)
                        .lock_exclusive()
                        .one(&txn)
                        .await?
                        .ok_or(AppError::NotFound)?;
                    let used_now = tenant_used_bytes(&txn, tenant_id).await?;
                    if let Some(q) = effective_quota(&tenant_q, &state.drive_config)
                        && used_now.saturating_add(actual_size as i64) > q
                    {
                        return Err(AppError::ContentTooLarge);
                    }
                    let model = drive_files::ActiveModel {
                        id: Set(file_id),
                        name: Set(name),
                        size: Set(actual_size as i64),
                        mime_type: Set(mime_type),
                        storage_type: Set(current_storage_type()),
                        storage_key: Set(storage_key.clone()),
                        tenant_id: Set(tenant_id),
                        project_id: Set(folder_project_id),
                        uploader_id: Set(auth.user_id),
                        folder_id: Set(folder_id),
                        created_at: Set(now.into()),
                        updated_at: Set(now.into()),
                    };
                    let saved = model.insert(&txn).await?;
                    txn.commit().await?;
                    Ok(saved)
                }
                .await;
                match result {
                    Ok(saved) => {
                        return Ok((StatusCode::CREATED, Json(saved.into())));
                    }
                    Err(e) => {
                        let _ = state.storage.delete(&storage_key).await;
                        return Err(e);
                    }
                }
            }
            Some("name") => {
                let text = field
                    .text()
                    .await
                    .map_err(|e| AppError::Internal(e.into()))?;
                if !text.is_empty() {
                    display_name = Some(text);
                }
            }
            Some("folder_id") => {
                let text = field
                    .text()
                    .await
                    .map_err(|e| AppError::Internal(e.into()))?;
                if !text.is_empty() {
                    folder_id =
                        Some(Uuid::parse_str(text.trim()).map_err(|_| AppError::BadRequest)?);
                }
            }
            _ => {}
        }
    }

    Err(AppError::BadRequest) // 'file' フィールドなし
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/files/{id}",
    tag = "Drive Files",
    summary = "ドライブファイルメタデータ取得",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("id" = Uuid, Path, description = "ファイルID"),
    ),
    responses(
        (status = 200, description = "ファイルメタデータ", body = DriveFileResponse),
        CrudErrors,
    )
)]
pub async fn get_file(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, id)): Path<(Uuid, Uuid)>,
) -> Result<Json<DriveFileResponse>, AppError> {
    auth.require_scope(Scope::ReadDrive)?;
    auth.ensure_tenant_access(&state, tenant_id, None).await?;
    let file = load_tenant_file(&state, tenant_id, id).await?;
    authorize_file_access(&state, &file, &auth).await?;
    Ok(Json(file.into()))
}

#[axum::debug_handler]
#[utoipa::path(
    patch,
    path = "/files/{id}",
    tag = "Drive Files",
    summary = "ドライブファイル更新",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("id" = Uuid, Path, description = "ファイルID"),
    ),
    request_body = UpdateFileRequest,
    responses(
        (status = 200, description = "更新されたファイル", body = DriveFileResponse),
        CrudErrors,
    )
)]
pub async fn update_file(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, id)): Path<(Uuid, Uuid)>,
    Valid(Json(payload)): Valid<Json<UpdateFileRequest>>,
) -> Result<Json<DriveFileResponse>, AppError> {
    auth.require_scope(Scope::WriteDrive)?;
    auth.ensure_tenant_access(&state, tenant_id, None).await?;

    // 取得・認可・更新をすべて同じロックの内側で行う。ロックの外で読むと、そのフォルダが
    // 別プロジェクトへ移動している最中に古い project_id を書いてしまう。
    // **認可もロックの内側でやり直す。** 外で認可すると、ロックを待っているあいだに
    // 別のリクエストがそのファイルを自分の入れないプロジェクトへ移していても、
    // 古いモデルと古い認可結果のまま名前を変えたりルートへ持ち出したりできる
    let txn = state.db.begin().await?;
    lock_tenant_drive(&txn, tenant_id).await?;

    let file = load_tenant_file_conn(&txn, tenant_id, id).await?;
    authorize_file_write(&txn, &file, &auth).await?;

    let mut active: drive_files::ActiveModel = file.into();

    if let Some(name) = payload.name {
        active.name = Set(name);
    }

    if let Some(folder_id) = payload.folder_id {
        let project_id = if let Some(fid) = folder_id {
            let folder = load_folder_in_tenant_conn(&txn, tenant_id, fid).await?;
            // 移動先の ACL。authorize_file_write は移動「元」しか見ないので、
            // これが無いと自分が入れないプロジェクトのフォルダへ送り込める
            // （送り込んだ側は以後そのファイルを読めなくなるが、相手の
            // プロジェクトに他人のファイルを置ける）。upload と同方針
            if let Some(project_id) = folder.project_id
                && !is_tenant_owner(&txn, tenant_id, auth.user_id).await?
                && !can_access_project(&txn, tenant_id, project_id, auth.user_id).await?
            {
                return Err(AppError::Forbidden);
            }
            folder.project_id
        } else {
            None
        };
        active.folder_id = Set(folder_id);
        active.project_id = Set(project_id);
    }

    active.updated_at = Set(Utc::now().into());
    let updated = active.update(&txn).await?;
    txn.commit().await?;
    Ok(Json(updated.into()))
}

#[axum::debug_handler]
#[utoipa::path(
    put,
    path = "/files/{id}/content",
    tag = "Drive Files",
    summary = "ドライブファイル本文更新",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("id" = Uuid, Path, description = "ファイルID"),
    ),
    request_body = UpdateFileContentRequest,
    responses(
        (status = 200, description = "更新されたファイル", body = DriveFileResponse),
        (
            status = 400,
            description = "テキストとして編集できない MIME タイプです",
            body = ServerError
        ),
        (
            status = 413,
            description = "本文がアップロード上限またはテナントのクォータを超えています",
            body = ServerError
        ),
        CrudErrors,
    )
)]
pub async fn update_file_content(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, id)): Path<(Uuid, Uuid)>,
    Json(payload): Json<UpdateFileContentRequest>,
) -> Result<Json<DriveFileResponse>, AppError> {
    auth.require_scope(Scope::WriteDrive)?;
    auth.ensure_tenant_access(&state, tenant_id, None).await?;

    let file = load_tenant_file(&state, tenant_id, id).await?;
    authorize_file_write(&state.db, &file, &auth).await?;

    if !is_editable_mime(&file.mime_type) {
        return Err(AppError::BadRequestDetail(format!(
            "テキストとして編集できない MIME タイプです: {}",
            file.mime_type
        )));
    }

    let bytes = Bytes::from(payload.content.into_bytes());
    let new_size = bytes.len() as u64;
    if new_size > state.drive_config.upload_max_bytes {
        return Err(AppError::ContentTooLarge);
    }

    // mime_type はファイルの不変属性なのでロック外で読んだ値で構わない。
    let mime_type = file.mime_type.clone();

    // 既存キーを直接上書きすると、書き込み途中で失敗したときに元の内容が壊れる
    // （ローカルバックエンドの upload は File::create で切り詰めるため）。
    // 新しいキーへ書いてから DB を差し替え、コミット後に旧キーを削除する。
    let new_storage_key = Uuid::new_v4().to_string();
    let stream: ByteStream = Box::pin(futures::stream::once(async move {
        Ok::<Bytes, StorageError>(bytes)
    }));
    state
        .storage
        .upload(&new_storage_key, stream, new_size, &mime_type)
        .await
        .map_err(storage_to_app_error)?;

    // テナント行と対象ファイル行を FOR UPDATE でロックし、クォータ確定チェックと
    // UPDATE をアトミックに実行する。旧サイズ・旧ストレージキーは必ずロック取得後の
    // 値を使う。ロック前に読んだ file の値で計算・削除すると、同一ファイルへの並行更新が
    // 互いの旧サイズを差し引いて実クォータ超過を許したり、同じ旧キーを二重削除して
    // 先行処理の中間キーを孤児化する。
    let new_key_for_update = new_storage_key.clone();
    let result: Result<(drive_files::Model, String), AppError> = async {
        let txn = state.db.begin().await?;
        let tenant = tenants::Entity::find_by_id(tenant_id)
            .lock_exclusive()
            .one(&txn)
            .await?
            .ok_or(AppError::NotFound)?;
        let locked = drive_files::Entity::find_by_id(id)
            .filter(drive_files::Column::TenantId.eq(tenant_id))
            .lock_exclusive()
            .one(&txn)
            .await?
            .ok_or(AppError::NotFound)?;
        let old_storage_key = locked.storage_key.clone();
        let used_now = tenant_used_bytes(&txn, tenant_id).await?;
        // 差し替え後の使用量 = 現在の合計 − ロック時点の旧サイズ + 新サイズ
        let projected = used_now
            .saturating_sub(locked.size)
            .saturating_add(new_size as i64);
        if let Some(quota) = effective_quota(&tenant, &state.drive_config)
            && projected > quota
        {
            return Err(AppError::ContentTooLarge);
        }

        let mut active: drive_files::ActiveModel = locked.into();
        active.size = Set(new_size as i64);
        active.storage_key = Set(new_key_for_update);
        active.storage_type = Set(current_storage_type());
        active.updated_at = Set(Utc::now().into());
        let saved = active.update(&txn).await?;
        txn.commit().await?;
        Ok((saved, old_storage_key))
    }
    .await;

    match result {
        Ok((saved, old_storage_key)) => {
            // DB は新キーを指しているので旧データは不要。削除に失敗しても更新自体は成立している。
            let _ = state.storage.delete(&old_storage_key).await;
            Ok(Json(saved.into()))
        }
        Err(e) => {
            let _ = state.storage.delete(&new_storage_key).await;
            Err(e)
        }
    }
}

#[axum::debug_handler]
#[utoipa::path(
    delete,
    path = "/files/{id}",
    tag = "Drive Files",
    summary = "ドライブファイル削除",
    params(
        ("tenant_id" = Uuid, Path, description = "テナントID"),
        ("id" = Uuid, Path, description = "ファイルID"),
    ),
    responses(
        (status = 204, description = "削除完了"),
        CrudErrors,
    )
)]
pub async fn delete_file(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((tenant_id, id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    auth.require_scope(Scope::WriteDrive)?;
    auth.ensure_tenant_access(&state, tenant_id, None).await?;

    let file = load_tenant_file(&state, tenant_id, id).await?;
    authorize_file_write(&state.db, &file, &auth).await?;
    let storage_key = file.storage_key.clone();
    drive_files::Entity::delete_by_id(id)
        .exec(&state.db)
        .await?;
    let _ = state.storage.delete(&storage_key).await;
    Ok(StatusCode::NO_CONTENT)
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/files/{id}/content",
    tag = "Drive Files",
    summary = "ドライブファイル内容配信",
    params(
        ("id" = Uuid, Path, description = "ファイルID"),
        ContentQuery,
    ),
    responses(
        (status = 200, description = "ファイルバイナリ"),
        CrudErrors,
    )
)]
pub async fn get_file_content(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
    Path(id): Path<Uuid>,
    Query(query): Query<ContentQuery>,
) -> Result<Response, AppError> {
    let file = drive_files::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    can_access_file_content(&state, &file, &auth, query.token.as_deref()).await?;

    let stream = state
        .storage
        .get_stream(&file.storage_key)
        .await
        .map_err(storage_to_app_error)?;

    let body = Body::from_stream(stream);
    // mime_type はアップロード時のクライアント申告に由来しうる（text/html も保存できる）。
    // inline で返すとブラウザが API と同一オリジンで内容を実行してしまうため、常に attachment で配信する。
    let disposition = format!("attachment; filename=\"{}\"", sanitize_filename(&file.name));

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, file.mime_type.as_str())
        .header(header::CONTENT_DISPOSITION, disposition)
        .body(body)
        .map_err(|e| AppError::Internal(e.into()))
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/usage",
    tag = "Drive Files",
    summary = "ドライブ使用量・クォータ取得",
    params(("tenant_id" = Uuid, Path, description = "テナントID")),
    responses(
        (status = 200, description = "使用量", body = DriveUsageResponse),
        CrudErrors,
    )
)]
pub async fn get_drive_usage(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(tenant_id): Path<Uuid>,
) -> Result<Json<DriveUsageResponse>, AppError> {
    auth.require_scope(Scope::ReadDrive)?;
    auth.ensure_tenant_access(&state, tenant_id, None).await?;

    let tenant = tenants::Entity::find_by_id(tenant_id)
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    let used_bytes = tenant_used_bytes(&state.db, tenant_id).await?;
    let quota = effective_quota(&tenant, &state.drive_config);

    Ok(Json(DriveUsageResponse {
        used_bytes,
        quota_bytes: quota,
        system_max_bytes: state.drive_config.system_max_bytes_opt(),
        unlimited: quota.is_none(),
    }))
}

#[axum::debug_handler]
#[utoipa::path(
    patch,
    path = "/quota",
    tag = "Drive Files",
    summary = "ドライブクォータ設定",
    params(("tenant_id" = Uuid, Path, description = "テナントID")),
    request_body = UpdateQuotaRequest,
    responses(
        (status = 200, description = "更新後の使用量情報", body = DriveUsageResponse),
        CrudErrors,
    )
)]
pub async fn update_drive_quota(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(tenant_id): Path<Uuid>,
    Json(payload): Json<UpdateQuotaRequest>,
) -> Result<Json<DriveUsageResponse>, AppError> {
    auth.require_scope(Scope::AdminTenant)?;
    let tenant = auth.ensure_tenant_owner(&state, tenant_id).await?;

    if let Some(quota_bytes) = payload.quota_bytes {
        if quota_bytes < 0 {
            return Err(AppError::BadRequest);
        }
        if let Some(system_max) = state.drive_config.system_max_bytes_opt()
            && quota_bytes > system_max
        {
            return Err(AppError::BadRequest);
        }
    }

    let mut active: tenants::ActiveModel = tenant.into();
    active.drive_quota_bytes = Set(payload.quota_bytes);
    let updated = active.update(&state.db).await?;

    let used_bytes = tenant_used_bytes(&state.db, tenant_id).await?;
    let quota = effective_quota(&updated, &state.drive_config);

    Ok(Json(DriveUsageResponse {
        used_bytes,
        quota_bytes: quota,
        system_max_bytes: state.drive_config.system_max_bytes_opt(),
        unlimited: quota.is_none(),
    }))
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c == '"' || c == '\r' || c == '\n' {
                '_'
            } else {
                c
            }
        })
        .collect()
}

fn storage_to_app_error(err: StorageError) -> AppError {
    match err {
        StorageError::InvalidKey => AppError::BadRequest,
        other => AppError::Internal(other.into()),
    }
}
