//! 要約コメント更新ジョブの合流（PR 単位の Redis フラグ）。
//!
//! 要約コメントはラウンドの作成時と指摘の状態遷移時に更新する。要求ごとに
//! ジョブを積むと、20 件の指摘を順に `fixed` にしただけで同じコメントへ
//! 20 回続けて書き込みに行き、GitHub の secondary rate limit に当たる。
//! 投稿の失敗はベストエフォート（握り潰してログに残す）なので、そこで落ちると
//! 要約は古いまま黙って取り残される。
//!
//! そこで「更新待ち」のフラグを PR 単位で立て、既に立っていればジョブを積まない。
//! ジョブは実行開始時にフラグを落としてから最新状態を読み直すので、合流されて
//! 積まれなかった更新も次の 1 回に必ず含まれる（仕様 §7）。
//!
//! 合流はジョブの本数を減らすだけで、同時に走ることは止められない。並行して
//! 走ると、先に古い状態を読んだジョブの書き込みが後から着き、コメントが
//! 巻き戻ったまま次の遷移まで直らない。実行区間も同じ単位でロックして直列化する。

use std::sync::LazyLock;

use uuid::Uuid;

use common::cache::redis::RedisConnection;

/// 更新待ちフラグの TTL。
///
/// 正常系ではジョブが実行開始時に落とすため、これはワーカーが落ちて
/// フラグが残った場合の保険（この時間で必ず明ける）。短すぎると合流が効かず、
/// 長すぎると更新が止まる時間が延びる。
pub const SUMMARY_PENDING_TTL_SECS: u64 = 5 * 60;

/// 実行区間のロックの TTL。
///
/// 正常系ではジョブの終了時に解放するため、これはワーカーが落ちてロックが
/// 残った場合の保険。
///
/// 短すぎると保持中に期限が切れ、塞いだはずの並行書き込みが戻ってくる。
/// ジョブ 1 回の GitHub API 往復は最悪ケースで 8 リクエスト（installation token /
/// PR メタ / コメント探索 5 ページ / 投稿）、HTTP クライアントのタイムアウトは
/// 1 リクエスト 30 秒なので 4 分。余裕を見て 10 分に置く。
pub const SUMMARY_LOCK_TTL_SECS: u64 = 10 * 60;

const KEY_SUMMARY_PENDING: &str = "github:review_summary:pending:";
const KEY_SUMMARY_LOCK: &str = "github:review_summary:lock:";

/// TTL を超えた古いジョブが、後から取得されたロックを削除しないよう、
/// 取得時に保存したトークンと一致するときだけ削除する。
static RELEASE_SUMMARY_LOCK_SCRIPT: LazyLock<redis::Script> = LazyLock::new(|| {
    redis::Script::new(
        r#"
        if redis.call('GET', KEYS[1]) == ARGV[1] then
            return redis.call('DEL', KEYS[1])
        end
        return 0
        "#,
    )
});

fn pending_key(project_id: Uuid, pr_number: i32) -> String {
    format!("{KEY_SUMMARY_PENDING}{project_id}:{pr_number}")
}

fn lock_key(project_id: Uuid, pr_number: i32) -> String {
    format!("{KEY_SUMMARY_LOCK}{project_id}:{pr_number}")
}

/// 更新待ちフラグを立てる（`SET key 1 NX EX SUMMARY_PENDING_TTL_SECS`）。
///
/// # Returns
/// * `Ok(true)` - 立てられた（ジョブを積む）
/// * `Ok(false)` - 既に更新待ちのジョブがある（積まずに合流させる）
///
/// # Errors
/// * Redis 接続・コマンド実行に失敗した場合
pub async fn try_mark_pending(
    redis: &RedisConnection,
    project_id: Uuid,
    pr_number: i32,
) -> Result<bool, anyhow::Error> {
    let mut conn = redis
        .conn
        .acquire()
        .await
        .map_err(|e| anyhow::anyhow!("redis acquire: {e}"))?;

    let marked: Option<String> = redis::cmd("SET")
        .arg(pending_key(project_id, pr_number))
        .arg("1")
        .arg("NX")
        .arg("EX")
        .arg(SUMMARY_PENDING_TTL_SECS)
        .query_async(&mut conn)
        .await
        .map_err(|e| anyhow::anyhow!("redis SET NX review summary pending: {e}"))?;

    Ok(marked.is_some())
}

/// 更新待ちフラグを落とす。
///
/// ジョブは**最新状態を読む前に**これを呼ぶ。順序を逆にすると、読んだ後・
/// 落とす前に起きた遷移が合流で捨てられ、その分が要約に出ないまま残る。
/// 先に落とす側の取りこぼしは「ジョブが 1 本余計に積まれる」だけで済む。
///
/// # Errors
/// * Redis 接続・コマンド実行に失敗した場合
pub async fn clear_pending(
    redis: &RedisConnection,
    project_id: Uuid,
    pr_number: i32,
) -> Result<(), anyhow::Error> {
    let mut conn = redis
        .conn
        .acquire()
        .await
        .map_err(|e| anyhow::anyhow!("redis acquire: {e}"))?;

    redis::cmd("DEL")
        .arg(pending_key(project_id, pr_number))
        .query_async::<()>(&mut conn)
        .await
        .map_err(|e| anyhow::anyhow!("redis DEL review summary pending: {e}"))?;

    Ok(())
}

/// 同じ PR の要約更新を直列化するロックを取る
/// （`SET key <random token> NX EX SUMMARY_LOCK_TTL_SECS`）。
///
/// # Returns
/// * `Ok(Some(token))` - 取れた（投稿してよい）
/// * `Ok(None)` - 同じ PR の更新が実行中（このジョブは投稿せず再試行へ回す）
///
/// # Errors
/// * Redis 接続・コマンド実行に失敗した場合
pub async fn try_acquire_update_lock(
    redis: &RedisConnection,
    project_id: Uuid,
    pr_number: i32,
) -> Result<Option<String>, anyhow::Error> {
    let mut conn = redis
        .conn
        .acquire()
        .await
        .map_err(|e| anyhow::anyhow!("redis acquire: {e}"))?;

    let token = Uuid::new_v4().to_string();
    let acquired: Option<String> = redis::cmd("SET")
        .arg(lock_key(project_id, pr_number))
        .arg(&token)
        .arg("NX")
        .arg("EX")
        .arg(SUMMARY_LOCK_TTL_SECS)
        .query_async(&mut conn)
        .await
        .map_err(|e| anyhow::anyhow!("redis SET NX review summary lock: {e}"))?;

    Ok(acquired.map(|_| token))
}

/// 取得時のトークンが現在値と一致するときだけロックを解放する。
///
/// 投稿に失敗したジョブでも解放する。残すと TTL のあいだ後続が全部再試行に
/// 回り、要約が止まるため。
///
/// # Errors
/// * Redis 接続・コマンド実行に失敗した場合
pub async fn release_update_lock(
    redis: &RedisConnection,
    project_id: Uuid,
    pr_number: i32,
    token: &str,
) -> Result<bool, anyhow::Error> {
    let mut conn = redis
        .conn
        .acquire()
        .await
        .map_err(|e| anyhow::anyhow!("redis acquire: {e}"))?;

    let deleted: i32 = RELEASE_SUMMARY_LOCK_SCRIPT
        .key(lock_key(project_id, pr_number))
        .arg(token)
        .invoke_async(&mut conn)
        .await
        .map_err(|e| anyhow::anyhow!("redis release review summary lock script: {e}"))?;

    Ok(deleted == 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// フラグ・ロックは PR ごとに独立している（別 PR の更新を巻き込まない）。
    #[test]
    fn keys_are_scoped_to_the_pull_request() {
        let project_id = Uuid::new_v4();
        for key in [pending_key, lock_key] {
            assert_ne!(key(project_id, 618), key(project_id, 619));
            assert_ne!(key(project_id, 618), key(Uuid::new_v4(), 618));
        }
        // 「更新待ち」と「実行中」は別のキー空間（片方が他方を消さない）
        assert_ne!(pending_key(project_id, 618), lock_key(project_id, 618));
    }
}
