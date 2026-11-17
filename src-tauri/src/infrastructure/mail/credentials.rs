//! 通用邮箱凭证；系统钥匙串保护密钥，磁盘仅保存密文。

use crate::application::error::PortError;
use crate::domain::value_objects::AccountId;
use serde::{Deserialize, Serialize};
use std::path::Path;
use crate::infrastructure::secret_store;

#[derive(Clone, Serialize, Deserialize)]
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

pub async fn save(
    credentials_dir: &Path,
    account_id: &AccountId,
    secret: &MailSecret,
) -> Result<(), PortError> {
    secret_store::save(credentials_dir, "mail", account_id.as_str(), secret)
}

pub async fn load(
    credentials_dir: &Path,
    account_id: &AccountId,
) -> Result<Option<MailSecret>, PortError> {
    secret_store::load(credentials_dir, "mail", account_id.as_str())
}

pub async fn delete(credentials_dir: &Path, account_id: &AccountId) -> Result<(), PortError> {
    secret_store::delete(credentials_dir, "mail", account_id.as_str())
}
