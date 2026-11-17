//! 通用邮箱凭证；系统钥匙串保护密钥，磁盘仅保存密文。

use crate::domain::entities::AccountMailTransport;
use crate::domain::value_objects::MailAuthKind;
use crate::infrastructure::error::InfraError;
use crate::infrastructure::secret_store;
use serde::{Deserialize, Serialize};
use std::path::Path;

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
    credential_ref: &str,
    secret: &MailSecret,
) -> Result<(), InfraError> {
    secret_store::save(credentials_dir, "mail", credential_ref, secret)
}

pub async fn load(
    credentials_dir: &Path,
    transport: &AccountMailTransport,
) -> Result<Option<MailSecret>, InfraError> {
    if transport.auth_kind != MailAuthKind::Password {
        return Err(InfraError::Auth {
            message: "通用 IMAP/SMTP V1 不支持 OAuth2，请使用密码或邮箱授权码".into(),
        });
    }
    let reference = transport
        .credential_ref
        .as_deref()
        .ok_or_else(|| InfraError::Auth {
            message: "账号缺少凭据引用，请重新登录".into(),
        })?;
    secret_store::load(credentials_dir, "mail", reference)
}

pub async fn delete(credentials_dir: &Path, credential_ref: &str) -> Result<(), InfraError> {
    secret_store::delete(credentials_dir, "mail", credential_ref)
}

pub async fn delete_for_transport(
    credentials_dir: &Path,
    transport: &AccountMailTransport,
) -> Result<(), InfraError> {
    if let Some(reference) = &transport.credential_ref {
        let kind = match transport.auth_kind {
            MailAuthKind::Password => "mail",
            MailAuthKind::OAuth2 => "graph",
        };
        secret_store::delete(credentials_dir, kind, reference)?;
    }
    Ok(())
}

/// 配置切换与启动回收共用跨进程锁；忙时明确返回，避免回收正在暂存的凭据。
pub fn lock(credentials_dir: &Path) -> Result<std::fs::File, InfraError> {
    std::fs::create_dir_all(credentials_dir).map_err(|_| InfraError::Storage {
        message: "创建凭据目录失败".into(),
    })?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(credentials_dir.join("mail-config.lock"))
        .map_err(|_| InfraError::Storage {
            message: "打开凭据配置锁失败".into(),
        })?;
    file.try_lock().map_err(|_| InfraError::Conflict {
        message: "账号配置正在更新，请稍后重试".into(),
    })?;
    Ok(file)
}
