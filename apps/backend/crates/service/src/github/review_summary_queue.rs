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

use uuid::Uuid;

use common::cache::redis::RedisConnection;

/// 更新待ちフラグの TTL。
///
/// 正常系ではジョブが実行開始時に落とすため、これはワーカーが落ちて
/// フラグが残った場合の保険（この時間で必ず明ける）。短すぎると合流が効かず、
/// 長すぎると更新が止まる時間が延びる。
pub const SUMMARY_PENDING_TTL_SECS: u64 = 5 * 60;

const KEY_SUMMARY_PENDING: &str = "github:review_summary:pending:";

fn pending_key(project_id: Uuid, pr_number: i32) -> String {
    format!("{KEY_SUMMARY_PENDING}{project_id}:{pr_number}")
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

#[cfg(test)]
mod tests {
    use super::*;

    /// フラグは PR ごとに独立している（別 PR の更新を巻き込まない）。
    #[test]
    fn pending_keys_are_scoped_to_the_pull_request() {
        let project_id = Uuid::new_v4();
        assert_ne!(pending_key(project_id, 618), pending_key(project_id, 619));
        assert_ne!(
            pending_key(project_id, 618),
            pending_key(Uuid::new_v4(), 618)
        );
    }
}
