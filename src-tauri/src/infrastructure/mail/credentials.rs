//! 通用邮箱凭证（密码 / 授权码 / OAuth access token 缓存）

use crate::application::error::PortError;
use crate::domain::value_objects::AccountId;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailSecret {
    pub username: String,
    /// 密码或应用专用授权码
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// OAuth2 access token（IMAP/SMTP XOAUTH2）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<i64>,
}

fn secret_path(credentials_dir: &Path, account_id: &AccountId) -> PathBuf {
    credentials_dir.join(format!("{}.mail-secret.json", account_id.as_str()))
}

pub async fn save(
    credentials_dir: &Path,
    account_id: &AccountId,
    secret: &MailSecret,
) -> Result<(), PortError> {
    let path = secret_path(credentials_dir, account_id);
    let json = serde_json::to_string_pretty(secret).map_err(|e| PortError::Storage {
        message: format!("序列化邮件凭证失败: {e}"),
    })?;
    tokio::fs::write(&path, json)
        .await
        .map_err(|e| PortError::Storage {
            message: format!("写入邮件凭证失败: {e}"),
        })?;
    Ok(())
}

pub async fn load(
    credentials_dir: &Path,
    account_id: &AccountId,
) -> Result<Option<MailSecret>, PortError> {
    let path = secret_path(credentials_dir, account_id);
    match tokio::fs::read_to_string(&path).await {
        Ok(raw) => {
            let secret = serde_json::from_str(&raw).map_err(|e| PortError::Storage {
                message: format!("解析邮件凭证失败: {e}"),
            })?;
            Ok(Some(secret))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(PortError::Storage {
            message: format!("读取邮件凭证失败: {e}"),
        }),
    }
}

pub async fn delete(credentials_dir: &Path, account_id: &AccountId) -> Result<(), PortError> {
    let path = secret_path(credentials_dir, account_id);
    match tokio::fs::remove_file(&path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(PortError::Storage {
            message: format!("删除邮件凭证失败: {e}"),
        }),
    }
}
