//! GitHub App インストールフロー用 CSRF state（Redis）。

use std::sync::LazyLock;

use anyhow::Context;
use auth_core::state::StateStore;
use base64::Engine;
use common::cache::redis::RedisConnection;
use rand::Rng;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::oauth::state::RedisStateStore;

const KEY_PREFIX: &str = "github_oauth_state:";
pub const TTL_SECS: u64 = 600;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubOAuthStatePayload {
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub user_id: Uuid,
    /// 再連携時は既存 installation を束縛。新規インストール時は `None`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installation_id: Option<i64>,
}

pub fn new_state_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub async fn store_state(
    redis: &RedisConnection,
    state: &str,
    payload: &GithubOAuthStatePayload,
) -> Result<(), anyhow::Error> {
    let value = serde_json::to_string(payload).context("serialize oauth state")?;
    RedisStateStore::new(redis)
        .store(&format!("{KEY_PREFIX}{state}"), &value, TTL_SECS)
        .await
}

/// 取得と削除を原子的に行う（再利用防止）。
pub async fn consume_state(
    redis: &RedisConnection,
    state: &str,
) -> Result<Option<GithubOAuthStatePayload>, anyhow::Error> {
    let Some(raw) = RedisStateStore::new(redis)
        .consume(&format!("{KEY_PREFIX}{state}"))
        .await?
    else {
        return Ok(None);
    };
    Ok(Some(
        serde_json::from_str(&raw).context("deserialize oauth state")?,
    ))
}

const SELECT_KEY_PREFIX: &str = "github_repo_select:";

/// リポジトリ選択用トークンの中身。callback で発行し、選択 API で照合する。
/// `installation_id` をリクエストで受け取らずここに束縛するのが要点。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoSelectPayload {
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub user_id: Uuid,
    pub installation_id: i64,
}

pub async fn store_select_token(
    redis: &RedisConnection,
    token: &str,
    payload: &RepoSelectPayload,
) -> Result<(), anyhow::Error> {
    let value = serde_json::to_string(payload).context("serialize repo select payload")?;
    RedisStateStore::new(redis)
        .store(&format!("{SELECT_KEY_PREFIX}{token}"), &value, TTL_SECS)
        .await
}

/// 一覧取得用。削除せずに読む（選択を確定するまで何度でも開ける）。
pub async fn peek_select_token(
    redis: &RedisConnection,
    token: &str,
) -> Result<Option<RepoSelectPayload>, anyhow::Error> {
    let mut conn = redis
        .conn
        .acquire()
        .await
        .map_err(|e| anyhow::anyhow!("redis acquire: {e}"))?;
    let raw: Option<String> = redis::cmd("GET")
        .arg(format!("{SELECT_KEY_PREFIX}{token}"))
        .query_async(&mut conn)
        .await
        .map_err(|e| anyhow::anyhow!("redis GET repo select: {e}"))?;
    let Some(raw) = raw else { return Ok(None) };
    Ok(Some(
        serde_json::from_str(&raw).context("deserialize repo select payload")?,
    ))
}

/// 選択トークンを原子的に取り出して消す（GET + PTTL + DEL）。
///
/// 残り TTL も一緒に返すのは、この後の DB 更新に失敗したときに
/// [`restore_select_token`] で有効期限を延ばさずに戻すため。
static CLAIM_SELECT_SCRIPT: LazyLock<redis::Script> = LazyLock::new(|| {
    redis::Script::new(
        r#"
        local value = redis.call('GET', KEYS[1])
        if not value then
            return nil
        end
        local pttl = redis.call('PTTL', KEYS[1])
        redis.call('DEL', KEYS[1])
        return { value, tostring(pttl) }
        "#,
    )
});

/// 連携を確定させる 1 リクエストだけを通す。
///
/// [`peek_select_token`] は消さずに読むので、同じトークンでの POST が同時に走ると
/// どちらも検証を通り抜けて別々のリポジトリを書き込める。DB を触る直前にこれで
/// 権利を取り、取れなかった側は弾く。
///
/// 戻り値は payload と、取り上げた時点の残り TTL（ミリ秒）。
pub async fn claim_select_token(
    redis: &RedisConnection,
    token: &str,
) -> Result<Option<(RepoSelectPayload, i64)>, anyhow::Error> {
    let mut conn = redis
        .conn
        .acquire()
        .await
        .map_err(|e| anyhow::anyhow!("redis acquire: {e}"))?;
    let claimed: Option<(String, String)> = CLAIM_SELECT_SCRIPT
        .key(format!("{SELECT_KEY_PREFIX}{token}"))
        .invoke_async(&mut conn)
        .await
        .map_err(|e| anyhow::anyhow!("redis claim repo select: {e}"))?;
    let Some((raw, pttl)) = claimed else {
        return Ok(None);
    };
    let payload: RepoSelectPayload =
        serde_json::from_str(&raw).context("deserialize repo select payload")?;
    let pttl = pttl.parse::<i64>().context("parse repo select pttl")?;
    Ok(Some((payload, pttl)))
}

/// 取り上げたトークンを、残り TTL のまま戻す。
///
/// 権利を取った後の DB 更新が落ちたときに使う。ここで [`store_select_token`] を
/// 呼ぶと有効期限が延びてしまうので、預かった残り TTL をそのまま書き戻す。
/// TTL が残っていなければ何もしない（放っておいても切れる）。
pub async fn restore_select_token(
    redis: &RedisConnection,
    token: &str,
    payload: &RepoSelectPayload,
    ttl_millis: i64,
) -> Result<(), anyhow::Error> {
    if ttl_millis < 0 {
        // PTTL が負を返すのは「キーはあるが期限が無い」ときで、この経路の書き方では起きない。
        // 起きたら戻せていないので痕跡を残す（0 は普通に期限切れなので黙って返す）。
        tracing::warn!(ttl_millis, "repo select token has no ttl; not restored");
        return Ok(());
    }
    if ttl_millis == 0 {
        return Ok(());
    }
    let value = serde_json::to_string(payload).context("serialize repo select payload")?;
    let mut conn = redis
        .conn
        .acquire()
        .await
        .map_err(|e| anyhow::anyhow!("redis acquire: {e}"))?;
    let _: () = redis::cmd("SET")
        .arg(format!("{SELECT_KEY_PREFIX}{token}"))
        .arg(value)
        .arg("PX")
        .arg(ttl_millis)
        .query_async(&mut conn)
        .await
        .map_err(|e| anyhow::anyhow!("redis restore repo select: {e}"))?;
    Ok(())
}

const PENDING_KEY_PREFIX: &str = "github_pending_install:";
/// 選択を放棄しても翌日までは同じインストールへ戻れるようにする。
pub const PENDING_TTL_SECS: u64 = 60 * 60 * 24;

/// 選択待ちのインストールをプロジェクト単位で覚えておく。
///
/// 連携レコードがまだ無いため、これが無いと再訪時の `verify_installation` が
/// 「新規インストールは state の TTL 内に作られたものだけ」判定で落ち、
/// GitHub から App を消すまで連携できなくなる。
pub async fn store_pending_installation(
    redis: &RedisConnection,
    project_id: Uuid,
    installation_id: i64,
) -> Result<(), anyhow::Error> {
    RedisStateStore::new(redis)
        .store(
            &format!("{PENDING_KEY_PREFIX}{project_id}"),
            &installation_id.to_string(),
            PENDING_TTL_SECS,
        )
        .await
}

/// 用済みになった控えを消す。
///
/// `installation_id` で控えている当人かを確かめてから消すこと。プロジェクトに枠は
/// 1 つしかないので、無関係なインストールの連携で消してしまうと、選択を放棄した
/// インストールへ戻る道が失われる。
pub async fn delete_pending_installation_if(
    redis: &RedisConnection,
    project_id: Uuid,
    installation_id: i64,
) -> Result<(), anyhow::Error> {
    let mut conn = redis
        .conn
        .acquire()
        .await
        .map_err(|e| anyhow::anyhow!("redis acquire: {e}"))?;
    let _: () = DELETE_PENDING_IF_SCRIPT
        .key(format!("{PENDING_KEY_PREFIX}{project_id}"))
        .arg(installation_id.to_string())
        .invoke_async(&mut conn)
        .await
        .map_err(|e| anyhow::anyhow!("redis delete pending installation: {e}"))?;
    Ok(())
}

/// 控えている installation が指定のものと一致するときだけ消す（比較と削除を原子的に）。
///
/// 読んでから消すまでの間に別のフローが新しい installation を控えると、
/// 古い処理が新しい控えを消してしまう。
static DELETE_PENDING_IF_SCRIPT: LazyLock<redis::Script> = LazyLock::new(|| {
    redis::Script::new(
        r#"
        if redis.call('GET', KEYS[1]) == ARGV[1] then
            redis.call('DEL', KEYS[1])
        end
        return 1
        "#,
    )
});

pub async fn peek_pending_installation(
    redis: &RedisConnection,
    project_id: Uuid,
) -> Result<Option<i64>, anyhow::Error> {
    let mut conn = redis
        .conn
        .acquire()
        .await
        .map_err(|e| anyhow::anyhow!("redis acquire: {e}"))?;
    let raw: Option<String> = redis::cmd("GET")
        .arg(format!("{PENDING_KEY_PREFIX}{project_id}"))
        .query_async(&mut conn)
        .await
        .map_err(|e| anyhow::anyhow!("redis GET pending installation: {e}"))?;
    raw.map(|v| v.parse::<i64>().context("parse pending installation id"))
        .transpose()
}
