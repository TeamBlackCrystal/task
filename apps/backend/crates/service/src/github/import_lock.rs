//! Issue 一括取り込みの重複防止（プロジェクト単位の Redis ロック）。
//!
//! `POST /github/import` は 202 を返した時点ではジョブを積んだだけなので、
//! 連打・リロード・別タブからの再実行でリポジトリの Issue 全件取得が
//! その回数だけ積まれる。タスク側は `apply_issue` が冪等なので重複は生まれないが、
//! Installation Access Token の GitHub API レート制限とワーカー時間は消費される。
//!
//! 画面側にも待ち時間を置いているが、それはブラウザのローカル状態にすぎない
//! （リロードや別タブでは消える）。実際に止めるのはここ。

use std::sync::LazyLock;

use uuid::Uuid;

use common::cache::redis::RedisConnection;

/// 取り込みロックの TTL。
///
/// 正常系ではジョブ完了時に [`release_import_slot`] で解放するため、これは
/// ワーカーが落ちてロックが解放されなかった場合の保険（この時間で必ず明ける）。
pub const IMPORT_LOCK_TTL_SECS: u64 = 15 * 60;

const KEY_IMPORT_LOCK: &str = "github:import:lock:";

/// TTL を超えた古いジョブが、後から取得されたロックを削除しないよう、
/// 取得時に保存したトークンと一致するときだけ削除する。
static RELEASE_IMPORT_SLOT_SCRIPT: LazyLock<redis::Script> = LazyLock::new(|| {
    redis::Script::new(
        r#"
        if redis.call('GET', KEYS[1]) == ARGV[1] then
            return redis.call('DEL', KEYS[1])
        end
        return 0
        "#,
    )
});

fn lock_key(project_id: Uuid) -> String {
    format!("{KEY_IMPORT_LOCK}{project_id}")
}

/// プロジェクト単位の取り込み枠を取得する
/// （`SET key <random token> NX EX IMPORT_LOCK_TTL_SECS`）。
///
/// # Returns
/// * `Ok(Some(token))` - 枠を取得できた（ジョブを積んでよい）
/// * `Ok(None)` - 同じプロジェクトの取り込みが待機中または実行中
///
/// # Errors
/// * Redis 接続・コマンド実行に失敗した場合
pub async fn try_acquire_import_slot(
    redis: &RedisConnection,
    project_id: Uuid,
) -> Result<Option<String>, anyhow::Error> {
    let mut conn = redis
        .conn
        .acquire()
        .await
        .map_err(|e| anyhow::anyhow!("redis acquire: {e}"))?;

    let token = Uuid::new_v4().to_string();
    let acquired: Option<String> = redis::cmd("SET")
        .arg(lock_key(project_id))
        .arg(&token)
        .arg("NX")
        .arg("EX")
        .arg(IMPORT_LOCK_TTL_SECS)
        .query_async(&mut conn)
        .await
        .map_err(|e| anyhow::anyhow!("redis SET NX import lock: {e}"))?;

    Ok(acquired.map(|_| token))
}

/// 取得時のトークンが現在値と一致するときだけ取り込み枠を解放する。
///
/// 失敗したジョブでも解放する。ロックを残すと、ユーザー自身のやり直しまで
/// TTL のあいだ塞いでしまい、取り込みが二重に走るより困るため。
///
/// # Errors
/// * Redis 接続・コマンド実行に失敗した場合
pub async fn release_import_slot(
    redis: &RedisConnection,
    project_id: Uuid,
    token: &str,
) -> Result<bool, anyhow::Error> {
    let mut conn = redis
        .conn
        .acquire()
        .await
        .map_err(|e| anyhow::anyhow!("redis acquire: {e}"))?;

    let deleted: i32 = RELEASE_IMPORT_SLOT_SCRIPT
        .key(lock_key(project_id))
        .arg(token)
        .invoke_async(&mut conn)
        .await
        .map_err(|e| anyhow::anyhow!("redis release import lock script: {e}"))?;

    Ok(deleted == 1)
}

/// 現在の取り込み枠の所有権トークンを取得する。枠がなければ `None`。
///
/// 連携解除では削除処理より前にこの値を控え、削除後に [`release_import_slot`] へ
/// 渡す。途中で TTL が切れて後続が枠を取り直しても、その後続ロックは削除しない。
pub async fn get_import_slot_token(
    redis: &RedisConnection,
    project_id: Uuid,
) -> Result<Option<String>, anyhow::Error> {
    let mut conn = redis
        .conn
        .acquire()
        .await
        .map_err(|e| anyhow::anyhow!("redis acquire: {e}"))?;

    let token: Option<String> = redis::cmd("GET")
        .arg(lock_key(project_id))
        .query_async(&mut conn)
        .await
        .map_err(|e| anyhow::anyhow!("redis GET import lock: {e}"))?;

    Ok(token)
}
