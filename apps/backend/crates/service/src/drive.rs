//! Drive クォータ・設定ヘルパー。

use std::env;
use std::path::Path;

use sea_orm::prelude::Uuid;
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, ExprTrait, QueryFilter, QuerySelect};

use crate::error::AppError;
use entity::{drive_files, tenants};

/// Drive 関連の環境変数設定。
#[derive(Clone, Debug)]
pub struct DriveConfig {
    pub upload_max_bytes: u64,
    /// `0` = 無制限（デフォルトクォータ未設定時）
    pub default_quota_bytes: i64,
    /// `0` = 天井なし
    pub system_max_quota_bytes: i64,
}

impl DriveConfig {
    pub fn from_env() -> Self {
        let upload_max_mb = env_u64("UPLOAD_MAX_SIZE_MB", 100);
        let default_quota_mb = env_i64("DRIVE_DEFAULT_QUOTA_MB", 10240);
        let system_max_mb = env_i64("DRIVE_SYSTEM_MAX_QUOTA_MB", 51200);

        Self {
            upload_max_bytes: upload_max_mb.saturating_mul(1024 * 1024),
            default_quota_bytes: mb_to_bytes(default_quota_mb),
            system_max_quota_bytes: mb_to_bytes(system_max_mb),
        }
    }

    pub fn system_max_bytes_opt(&self) -> Option<i64> {
        if self.system_max_quota_bytes == 0 {
            None
        } else {
            Some(self.system_max_quota_bytes)
        }
    }
}

fn env_u64(key: &str, default: u64) -> u64 {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_i64(key: &str, default: i64) -> i64 {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn mb_to_bytes(mb: i64) -> i64 {
    if mb == 0 {
        0
    } else {
        mb.saturating_mul(1024 * 1024)
    }
}

/// テナントの有効クォータ（バイト）。`None` = 無制限。
/// システム上限（`system_max_quota_bytes`）が設定されている場合は常にその値でキャップする。
pub fn effective_quota(tenant: &tenants::Model, config: &DriveConfig) -> Option<i64> {
    let requested = tenant
        .drive_quota_bytes
        .unwrap_or(config.default_quota_bytes);
    let requested_opt = if requested == 0 {
        None
    } else {
        Some(requested)
    };
    let system_max_opt = config.system_max_bytes_opt();
    match (requested_opt, system_max_opt) {
        (Some(q), Some(max)) => Some(std::cmp::Ord::min(q, max)),
        (None, Some(max)) => Some(max),
        (Some(q), None) => Some(q),
        (None, None) => None,
    }
}

pub async fn tenant_used_bytes<C: ConnectionTrait>(
    db: &C,
    tenant_id: Uuid,
) -> Result<i64, AppError> {
    // Postgres の SUM(bigint) は NUMERIC を返すため、そのまま i64 で受け取ると
    // 「NUMERIC は INT8 と互換でない」というデコードエラーになる。行が 0 件のときは
    // SUM が NULL になり素通りするので、テナントにファイルが 1 件でもあると再現する。
    // BIGINT へキャストしてから受け取る。
    let sum = drive_files::Entity::find()
        .filter(drive_files::Column::TenantId.eq(tenant_id))
        .select_only()
        .column_as(
            sea_orm::sea_query::Expr::col(drive_files::Column::Size)
                .sum()
                .cast_as("bigint"),
            "total",
        )
        .into_tuple::<Option<i64>>()
        .one(db)
        .await?;

    Ok(sum.flatten().unwrap_or(0))
}

pub fn current_storage_type() -> entity::drive_files::StorageType {
    match env::var("STORAGE_BACKEND")
        .unwrap_or_else(|_| "local".into())
        .as_str()
    {
        "s3" => entity::drive_files::StorageType::S3,
        _ => entity::drive_files::StorageType::Local,
    }
}

/// テナントオーナー判定（drive_files / drive_folders の共通ヘルパー）。
pub async fn is_tenant_owner<C: ConnectionTrait>(
    db: &C,
    tenant_id: Uuid,
    user_id: Uuid,
) -> Result<bool, AppError> {
    let tenant = tenants::Entity::find_by_id(tenant_id)
        .one(db)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(tenant.owner_id == user_id)
}

/// `mime_guess` がソースコードとして解決できない拡張子の上書き表。
///
/// `mime_guess` は `.ts` を MPEG-TS（`video/vnd.dlna.mpeg-tts`）に解決し、
/// `.go` や `.rb` などは未定義のため `application/octet-stream` に落ちる。
/// そのままだとテキストとして扱えず編集エンドポイントが弾いてしまうため、
/// ここで `text/*` に寄せる。`mime_guess` が既に text 系を返す拡張子
/// （`.rs` / `.py` / `.c` / `.lua` など）は重複させない。
const SOURCE_MIME_OVERRIDES: &[(&str, &str)] = &[
    ("cjs", "text/javascript"),
    ("clj", "text/x-clojure"),
    ("dart", "text/x-dart"),
    ("erl", "text/x-erlang"),
    ("ex", "text/x-elixir"),
    ("exs", "text/x-elixir"),
    ("go", "text/x-go"),
    ("graphql", "text/x-graphql"),
    ("java", "text/x-java"),
    ("kt", "text/x-kotlin"),
    ("nim", "text/x-nim"),
    ("proto", "text/x-protobuf"),
    ("rb", "text/x-ruby"),
    ("scala", "text/x-scala"),
    ("svelte", "text/x-svelte"),
    ("swift", "text/x-swift"),
    ("ts", "text/typescript"),
    ("tsx", "text/typescript"),
    ("vue", "text/x-vue"),
    ("zig", "text/x-zig"),
];

/// 上書き表に載っているソースコード拡張子の MIME を返す。
///
/// クライアントが申告する Content-Type よりこちらを優先したい場面があるため、
/// [`guess_mime`] とは別に公開している。ブラウザは `.ts` を `video/mp2t` の
/// ように申告することがあり、そのまま保存すると本文編集エンドポイントが弾く。
pub fn source_mime_override(filename: &str) -> Option<&'static str> {
    let extension = Path::new(filename)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)?;

    SOURCE_MIME_OVERRIDES
        .iter()
        .find(|(ext, _)| *ext == extension)
        .map(|(_, mime)| *mime)
}

pub fn guess_mime(filename: &str) -> String {
    if let Some(mime) = source_mime_override(filename) {
        return mime.to_string();
    }

    mime_guess::from_path(filename)
        .first_raw()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "application/octet-stream".into())
}

/// エディタで本文を差し替えられるテキスト系 MIME か判定する。
///
/// `text/*` に加えて、構造化サフィックス（`+json` / `+xml`。`application/ld+json` や
/// `image/svg+xml` が該当）と、テキストでありながら `application/*` に置かれている
/// 形式を許可する。`text/plain; charset=utf-8` のようなパラメータ付きの値も受け付ける。
pub fn is_editable_mime(mime: &str) -> bool {
    let essence = mime
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    if essence.starts_with("text/") || essence.ends_with("+json") || essence.ends_with("+xml") {
        return true;
    }

    matches!(
        essence.as_str(),
        "application/ecmascript"
            | "application/graphql"
            | "application/javascript"
            | "application/json"
            | "application/toml"
            | "application/x-httpd-php"
            | "application/x-sh"
            | "application/x-sql"
            | "application/x-toml"
            | "application/x-yaml"
            | "application/xml"
            | "application/yaml"
    )
}

/// プロジェクト配下のリソース（ファイル・フォルダ）にアクセスできるか。
///
/// ファイル ID だけで引ける配信経路があるため、プロジェクトの所属だけを見ると
/// テナント境界を越えられる。テナントに入れることを先に確かめてから、
/// メンバー未指定のプロジェクトはテナント全体に開放する（#568）。
///
/// drive_files と drive_folders の両ハンドラーが同じ判定を使うので service に置く。
pub async fn can_access_project<C: ConnectionTrait>(
    db: &C,
    tenant_id: Uuid,
    project_id: Uuid,
    user_id: Uuid,
) -> Result<bool, AppError> {
    if is_tenant_owner(db, tenant_id, user_id).await? {
        return Ok(true);
    }
    if !crate::access::is_tenant_member(db, tenant_id, user_id).await? {
        return Ok(false);
    }
    crate::access::project_is_open_or_member(db, project_id, user_id).await
}

/// テナントの Drive 階層を直列化するアドバイザリロック。
///
/// 移動・作成をトランザクションへ入れるだけでは足りない。ACL と親の `project_id` を
/// 読んでから書くまでの間に別のリクエストが割り込めるため、たとえば一般フォルダを
/// プロジェクト配下へ移動している最中にそのフォルダへ子を作ると、子は移動前の
/// `project_id = NULL` を継承したまま残る。同期の対象を集めた後に挿入されれば
/// プロジェクト配下に一般フォルダが残り、直したはずの ACL 漏れが再発する。
///
/// フォルダの作成・移動、ファイルの作成・移動をテナント単位で直列化して塞ぐ。
/// キーはテナント UUID の先頭 8 バイト。別テナントと衝突しても余分に直列化される
/// だけで、誤って通ることはない。
///
/// `pg_advisory_xact_lock` はトランザクション終了で自動的に解放される
/// （明示的な unlock を忘れて詰まらせない）。
pub async fn lock_tenant_drive<C: ConnectionTrait>(
    txn: &C,
    tenant_id: Uuid,
) -> Result<(), AppError> {
    let key = tenant_drive_lock_key(tenant_id);
    common::db::execute_bound(txn, "SELECT pg_advisory_xact_lock(?)", vec![key.into()]).await?;
    Ok(())
}

/// [`lock_tenant_drive`] が使うロックキー。テスト側から同じ鍵で押さえるために公開する。
pub fn tenant_drive_lock_key(tenant_id: Uuid) -> i64 {
    let head: [u8; 8] = tenant_id.as_bytes()[..8]
        .try_into()
        .expect("uuid は 16 バイト");
    i64::from_be_bytes(head)
}

/// プロジェクト作成時に自動生成されたルートフォルダか。
///
/// `create_project` が「1 プロジェクト = 1 ルートフォルダ」で作るもの（`project_id` 付き・
/// 親なし）。これを直接削除・移動できると、プロジェクトとドライブの対応が壊れる。
/// 破棄はプロジェクト削除の CASCADE に任せる。
pub fn is_project_root_folder(folder: &entity::drive_folders::Model) -> bool {
    folder.project_id.is_some() && folder.parent_id.is_none()
}

/// フォルダの子孫フォルダ ID を集める（自身は含まない）。
///
/// 階層は通常浅い（3〜5 段）が、深さを前提にしない反復で辿る。移動時に配下の
/// `project_id` を揃えるために使う。
pub async fn descendant_folder_ids<C: ConnectionTrait>(
    conn: &C,
    folder_id: Uuid,
) -> Result<Vec<Uuid>, AppError> {
    let mut collected = Vec::new();
    let mut frontier = vec![folder_id];

    while !frontier.is_empty() {
        let children: Vec<Uuid> = entity::drive_folders::Entity::find()
            .filter(entity::drive_folders::Column::ParentId.is_in(frontier.clone()))
            .all(conn)
            .await?
            .into_iter()
            .map(|folder| folder.id)
            .collect();

        // 循環は validate_parent_folder が作らせないが、万一混ざっても止まるように
        // 既知の ID は辿らない
        frontier = children
            .into_iter()
            .filter(|id| !collected.contains(id) && *id != folder_id)
            .collect();
        collected.extend(frontier.iter().copied());
    }

    Ok(collected)
}

/// フォルダとその配下（フォルダ・ファイル）の `project_id` を揃える。
///
/// 移動でプロジェクト境界を跨いだとき、配下だけ古い `project_id` を持つと
/// 「プロジェクトファイルなのに別プロジェクトの配下に居る」状態になり、
/// 配信の認可（`project_id` で判定する）が階層と食い違う。
/// 呼び出し側がトランザクションを渡し、移動と同一トランザクションで揃える。
pub async fn sync_subtree_project_id<C: ConnectionTrait>(
    txn: &C,
    folder_id: Uuid,
    project_id: Option<Uuid>,
) -> Result<(), AppError> {
    let mut folder_ids = vec![folder_id];
    folder_ids.extend(descendant_folder_ids(txn, folder_id).await?);

    entity::drive_folders::Entity::update_many()
        .col_expr(
            entity::drive_folders::Column::ProjectId,
            sea_orm::sea_query::Expr::value(project_id),
        )
        .filter(entity::drive_folders::Column::Id.is_in(folder_ids.clone()))
        .exec(txn)
        .await?;

    entity::drive_files::Entity::update_many()
        .col_expr(
            entity::drive_files::Column::ProjectId,
            sea_orm::sea_query::Expr::value(project_id),
        )
        .filter(entity::drive_files::Column::FolderId.is_in(folder_ids))
        .exec(txn)
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `mime_guess` 単体では誤り（`.ts` = MPEG-TS）または未定義になる拡張子が、
    /// 上書き表によってテキストとして解決されることの回帰ガード。
    #[test]
    fn guess_mime_resolves_source_extensions_as_text() {
        for name in [
            "main.ts",
            "App.tsx",
            "main.go",
            "app.rb",
            "Main.java",
            "main.kt",
            "schema.graphql",
            "Component.vue",
        ] {
            let mime = guess_mime(name);
            assert!(
                is_editable_mime(&mime),
                "{name} は編集可能なテキストとして解決されるべき (実際: {mime})"
            );
        }
        assert_eq!(guess_mime("main.ts"), "text/typescript");
        assert_eq!(guess_mime("MAIN.GO"), "text/x-go");
    }

    /// 上書き表に無い拡張子は従来どおり `mime_guess` の結果を返す。
    #[test]
    fn guess_mime_falls_back_to_mime_guess() {
        assert_eq!(guess_mime("lib.rs"), "text/x-rust");
        assert_eq!(guess_mime("notes.md"), "text/markdown");
        assert_eq!(guess_mime("data.json"), "application/json");
        assert_eq!(guess_mime("photo.png"), "image/png");
        assert_eq!(guess_mime("archive"), "application/octet-stream");
    }

    #[test]
    fn is_editable_mime_accepts_text_and_text_like_types() {
        for mime in [
            "text/plain",
            "text/plain; charset=utf-8",
            "  TEXT/Markdown  ",
            "text/x-rust",
            "application/json",
            "application/ld+json",
            "image/svg+xml",
            "application/x-sh",
            "application/x-yaml",
        ] {
            assert!(is_editable_mime(mime), "{mime} は編集可能であるべき");
        }
    }

    #[test]
    fn is_editable_mime_rejects_binary_types() {
        for mime in [
            "image/png",
            "application/pdf",
            "application/octet-stream",
            "video/vnd.dlna.mpeg-tts",
            "application/zip",
            "",
        ] {
            assert!(!is_editable_mime(mime), "{mime} は編集不可であるべき");
        }
    }
}
