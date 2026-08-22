//! Issue 一括取り込みの重複防止（プロジェクト単位の Redis ロック）。
//!
//! `POST /github/import` は 202 を返した時点ではジョブを積んだだけなので、
//! 連打・リロード・別タブからの再実行でリポジトリの Issue 全件取得が
//! その回数だけ積まれる。タスク側は `apply_issue` が冪等なので重複は生まれないが、
//! Installation Access Token の GitHub API レート制限とワーカー時間は消費される。
//!
//! 画面側にも待ち時間を置いているが、それはブラウザのローカル状態にすぎない
//! （リロードや別タブでは消える）。実際に止めるのはここ。

use uuid::Uuid;

use common::cache::redis::RedisConnection;

/// 取り込みロックの TTL。
///
/// 正常系ではジョブ完了時に [`release_import_slot`] で解放するため、これは
/// ワーカーが落ちてロックが解放されなかった場合の保険（この時間で必ず明ける）。
pub const IMPORT_LOCK_TTL_SECS: u64 = 15 * 60;

const KEY_IMPORT_LOCK: &str = "github:import:lock:";

fn lock_key(project_id: Uuid) -> String {
    format!("{KEY_IMPORT_LOCK}{project_id}")
}

/// プロジェクト単位の取り込み枠を取得する（`SET key 1 NX EX IMPORT_LOCK_TTL_SECS`）。
///
/// # Returns
/// * `Ok(true)` - 枠を取得できた（ジョブを積んでよい）
/// * `Ok(false)` - 同じプロジェクトの取り込みが待機中または実行中
///
/// # Errors
/// * Redis 接続・コマンド実行に失敗した場合
pub async fn try_acquire_import_slot(
    redis: &RedisConnection,
    project_id: Uuid,
) -> Result<bool, anyhow::Error> {
    let mut conn = redis
        .conn
        .acquire()
        .await
        .map_err(|e| anyhow::anyhow!("redis acquire: {e}"))?;

    let acquired: Option<String> = redis::cmd("SET")
        .arg(lock_key(project_id))
        .arg("1")
        .arg("NX")
        .arg("EX")
        .arg(IMPORT_LOCK_TTL_SECS)
        .query_async(&mut conn)
        .await
        .map_err(|e| anyhow::anyhow!("redis SET NX import lock: {e}"))?;

    Ok(acquired.is_some())
}

/// 取り込み枠を解放する（`DEL`）。存在しなくても成功。
///
/// 失敗したジョブでも解放する。ロックを残すと、ユーザー自身のやり直しまで
/// TTL のあいだ塞いでしまい、取り込みが二重に走るより困るため。
///
/// # Errors
/// * Redis 接続・コマンド実行に失敗した場合
pub async fn release_import_slot(
    redis: &RedisConnection,
    project_id: Uuid,
) -> Result<(), anyhow::Error> {
    let mut conn = redis
        .conn
        .acquire()
        .await
        .map_err(|e| anyhow::anyhow!("redis acquire: {e}"))?;

    let _: () = redis::cmd("DEL")
        .arg(lock_key(project_id))
        .query_async(&mut conn)
        .await
        .map_err(|e| anyhow::anyhow!("redis DEL import lock: {e}"))?;

    Ok(())
}
