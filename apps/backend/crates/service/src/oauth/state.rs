//! OAuth state の payload 型と、Redis を使った [`StateStore`] 実装。

use auth_core::state::StateStore;
use common::cache::redis::RedisConnection;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use auth_core::state::{
    RedirectValidationError, STATE_TTL_SECS, build_frontend_oauth_error_redirect,
    build_frontend_redirect, sanitize_redirect_path,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthStatePayload {
    /// OAuth フロー開始時のプロバイダー slug（callback で照合して CSRF を防ぐ）
    pub provider: String,
    pub code_verifier: String,
    pub redirect_after: String,
    /// プロバイダーがエラーを返したときの戻り先。成功用 `redirect_after` とは別に保持し、
    /// OAuth ボタンを描画するページ（signin/signup）へ戻してエラーを表示するために使う。
    pub error_redirect_after: String,
    /// アカウント連携時のログイン済みユーザー ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_user_id: Option<Uuid>,
    /// GitLab self-hosted のインスタンス URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_url: Option<String>,
}

/// Redis を使う [`StateStore`] 実装。
///
/// `RedisConnection` も `StateStore` もこのクレートの外の型なので、孤児ルール回避のため
/// newtype でくるむ。
#[derive(Clone)]
pub struct RedisStateStore(RedisConnection);

impl RedisStateStore {
    pub fn new(redis: &RedisConnection) -> Self {
        Self(redis.clone())
    }
}

#[async_trait::async_trait]
impl StateStore for RedisStateStore {
    async fn store(&self, key: &str, value: &str, ttl_secs: u64) -> Result<(), anyhow::Error> {
        let mut conn = self
            .0
            .conn
            .acquire()
            .await
            .map_err(|e| anyhow::anyhow!("redis acquire: {e}"))?;

        let _: () = redis::cmd("SET")
            .arg(key)
            .arg(value)
            .arg("EX")
            .arg(ttl_secs)
            .query_async(&mut conn)
            .await
            .map_err(|e| anyhow::anyhow!("redis SET oauth state: {e}"))?;

        Ok(())
    }

    async fn consume(&self, key: &str) -> Result<Option<String>, anyhow::Error> {
        let mut conn = self
            .0
            .conn
            .acquire()
            .await
            .map_err(|e| anyhow::anyhow!("redis acquire: {e}"))?;

        // GETDEL: 取得と削除を原子的に行い、state を使い捨てにする。
        redis::cmd("GETDEL")
            .arg(key)
            .query_async(&mut conn)
            .await
            .map_err(|e| anyhow::anyhow!("redis GETDEL oauth state: {e}"))
    }
}

pub async fn store_state(
    redis: &RedisConnection,
    state: &str,
    payload: &OAuthStatePayload,
) -> Result<(), anyhow::Error> {
    auth_core::state::store_state(&RedisStateStore::new(redis), state, payload).await
}

/// state を検証して取得し、Redis から即削除する（使い捨て）。
pub async fn consume_state(
    redis: &RedisConnection,
    state: &str,
) -> Result<Option<OAuthStatePayload>, anyhow::Error> {
    auth_core::state::consume_state(&RedisStateStore::new(redis), state).await
}
