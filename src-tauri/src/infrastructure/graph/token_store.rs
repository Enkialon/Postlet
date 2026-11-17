//! Graph token 加密存储；不读取旧的明文开发凭据。

use crate::infrastructure::error::InfraError;
use crate::infrastructure::secret_store;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// 持久化的 OAuth token 缓存（禁止写入业务 SQLite）
#[derive(Clone, Serialize, Deserialize)]
pub struct TokenCache {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at_ms: i64,
    pub scopes: Vec<String>,
    pub home_account_id: Option<String>,
    pub client_id: String,
    pub tenant: String,
}

pub fn save(
    credentials_dir: &Path,
    credential_ref: &str,
    cache: &TokenCache,
) -> Result<(), InfraError> {
    secret_store::save(credentials_dir, "graph", credential_ref, cache)
}

pub fn load(
    credentials_dir: &Path,
    credential_ref: &str,
) -> Result<Option<TokenCache>, InfraError> {
    secret_store::load(credentials_dir, "graph", credential_ref)
}

/// access token 是否仍可用（提前 2 分钟视为过期）
pub fn is_access_token_fresh(cache: &TokenCache, now_ms: i64) -> bool {
    cache.expires_at_ms > now_ms + 120_000
}
