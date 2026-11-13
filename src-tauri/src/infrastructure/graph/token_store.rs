//! Token 文件存储：`credentials/{account_id}.json`

use crate::application::error::PortError;
use crate::domain::value_objects::AccountId;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// 持久化的 OAuth token 缓存（禁止写入业务 SQLite）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenCache {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at_ms: i64,
    pub scopes: Vec<String>,
    pub home_account_id: Option<String>,
    pub client_id: String,
    pub tenant: String,
}

fn token_path(credentials_dir: &Path, account_id: &AccountId) -> PathBuf {
    credentials_dir.join(format!("{}.json", account_id.as_str()))
}

pub fn save(credentials_dir: &Path, account_id: &AccountId, cache: &TokenCache) -> Result<(), PortError> {
    fs::create_dir_all(credentials_dir).map_err(|e| PortError::Storage {
        message: format!("创建 credentials 目录失败: {e}"),
    })?;
    let path = token_path(credentials_dir, account_id);
    let raw = serde_json::to_string_pretty(cache).map_err(|e| PortError::Storage {
        message: format!("序列化 token 失败: {e}"),
    })?;
    fs::write(&path, raw).map_err(|e| PortError::Storage {
        message: format!("写入凭证失败: {e}"),
    })?;
    Ok(())
}

pub fn load(credentials_dir: &Path, account_id: &AccountId) -> Result<Option<TokenCache>, PortError> {
    let path = token_path(credentials_dir, account_id);
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path).map_err(|e| PortError::Storage {
        message: format!("读取凭证失败: {e}"),
    })?;
    let cache = serde_json::from_str(&raw).map_err(|_| PortError::Auth {
        message: "本地凭证已损坏，请重新登录".into(),
    })?;
    Ok(Some(cache))
}

pub fn delete(credentials_dir: &Path, account_id: &AccountId) -> Result<(), PortError> {
    let path = token_path(credentials_dir, account_id);
    if path.exists() {
        fs::remove_file(&path).map_err(|e| PortError::Storage {
            message: format!("删除凭证失败: {e}"),
        })?;
    }
    Ok(())
}

pub fn list_account_ids(credentials_dir: &Path) -> Result<Vec<AccountId>, PortError> {
    if !credentials_dir.exists() {
        return Ok(vec![]);
    }
    let mut ids = Vec::new();
    let entries = fs::read_dir(credentials_dir).map_err(|e| PortError::Storage {
        message: format!("读取 credentials 目录失败: {e}"),
    })?;
    for entry in entries {
        let entry = entry.map_err(|e| PortError::Storage {
            message: format!("遍历 credentials 失败: {e}"),
        })?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            ids.push(AccountId::new(stem.to_owned()));
        }
    }
    ids.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    Ok(ids)
}

/// access token 是否仍可用（提前 2 分钟视为过期）
pub fn is_access_token_fresh(cache: &TokenCache, now_ms: i64) -> bool {
    cache.expires_at_ms > now_ms + 120_000
}
