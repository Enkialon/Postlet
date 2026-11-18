//! 在已认证的连接中完成多文件夹同步。

use super::client::{list_capabilities, ImapSession};
use super::sync::{sync_folder, FolderSyncRequest, FolderSyncResult};
use crate::infrastructure::error::InfraError;
use crate::infrastructure::mail::operation::MailOperation;

#[derive(Debug, Clone)]
pub struct ImapSyncBatch {
    pub idle_supported: bool,
}

/// 每完成一个文件夹立即回调交付，调用方可在下一夹开始前提交并通知 UI。
pub fn sync_account_blocking(
    mut session: ImapSession,
    requests: &[FolderSyncRequest],
    page_limit: u32,
    operation: &MailOperation,
    mut on_folder: impl FnMut(FolderSyncResult),
) -> Result<ImapSyncBatch, InfraError> {
    operation.checkpoint()?;
    let (idle_supported, _) = list_capabilities(&mut session)?;

    // 无勾选文件夹时不偷偷回落 INBOX；由上层 resolve_mail_folders 决定范围。
    if requests.is_empty() {
        let _ = session.logout();
        return Ok(ImapSyncBatch { idle_supported });
    }

    for req in requests {
        operation.checkpoint()?;
        match sync_folder(&mut session, req, page_limit, operation) {
            Ok(result) => on_folder(result),
            Err(error) => {
                // 取消/超时必须停止；其他单夹失败只影响本夹，不丢弃已交付批次。
                if operation.checkpoint().is_err() {
                    let _ = session.logout();
                    return Err(error);
                }
                on_folder(FolderSyncResult::unavailable(
                    req,
                    format!("IMAP 同步失败: {error}"),
                ));
            }
        }
    }
    let _ = session.logout();
    Ok(ImapSyncBatch { idle_supported })
}
