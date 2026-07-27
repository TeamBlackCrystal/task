//! GitHub App インストールフロー用 CSRF state（Redis）。

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
