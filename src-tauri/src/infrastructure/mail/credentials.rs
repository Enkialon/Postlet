//! 通用邮箱凭证；系统钥匙串保护密钥，磁盘仅保存密文。

use crate::domain::entities::{AccountMailTransport, PasswordAuthConfig, ReceiveConfig, SendConfig};
use crate::domain::value_objects::CredentialRef;
use crate::infrastructure::error::InfraError;
use crate::infrastructure::secret_store;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Serialize, Deserialize)]
pub struct MailSecret { pub password: String }

pub async fn save(credentials_dir: &Path, reference: &CredentialRef, secret: &MailSecret) -> Result<(), InfraError> {
    secret_store::save(credentials_dir, "mail", reference.as_str(), secret)
}

pub async fn load(credentials_dir: &Path, auth: &PasswordAuthConfig) -> Result<Option<MailSecret>, InfraError> {
    secret_store::load(credentials_dir, "mail", auth.credential_ref().as_str())
}

pub async fn delete(credentials_dir: &Path, reference: &CredentialRef) -> Result<(), InfraError> {
    secret_store::delete(credentials_dir, "mail", reference.as_str())
}

pub async fn delete_for_transport(credentials_dir: &Path, transport: &AccountMailTransport) -> Result<(), InfraError> {
    let receive_ref = transport.receive.credential_ref();
    let send_ref = transport.send.credential_ref();
    let receive_kind = match &transport.receive {
        ReceiveConfig::Graph(_) => "graph",
        ReceiveConfig::Imap(_) => "mail",
    };
    secret_store::delete(credentials_dir, receive_kind, receive_ref.as_str())?;
    // 共用引用只删一次，避免误把同一秘密当成两侧独立文件。
    if send_ref != receive_ref {
        let send_kind = match &transport.send {
            SendConfig::Graph(_) => "graph",
            SendConfig::Smtp(_) => "mail",
        };
        secret_store::delete(credentials_dir, send_kind, send_ref.as_str())?;
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
