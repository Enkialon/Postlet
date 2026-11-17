//! 在已认证的连接中完成多文件夹同步。

use super::client::{list_capabilities, ImapSession};
use super::sync::{sync_folder, FolderSyncRequest, FolderSyncResult};
use crate::infrastructure::error::InfraError;

#[derive(Debug, Clone)]
pub struct ImapSyncBatch {
    pub idle_supported: bool,
    pub folders: Vec<FolderSyncResult>,
}

pub fn sync_account_blocking(
    mut session: ImapSession,
    requests: &[FolderSyncRequest],
    page_limit: u32,
) -> Result<ImapSyncBatch, InfraError> {
    let (idle_supported, _) = list_capabilities(&mut session)?;

    // 无勾选文件夹时不偷偷回落 INBOX；由上层 resolve_mail_folders 决定范围。
    if requests.is_empty() {
        let _ = session.logout();
        return Ok(ImapSyncBatch {
            idle_supported,
            folders: Vec::new(),
        });
    }

    let mut folders = Vec::new();
    for req in requests {
        let result = sync_folder(&mut session, req, page_limit)?;
        folders.push(result);
    }
    let _ = session.logout();
    Ok(ImapSyncBatch {
        idle_supported,
        folders,
    })
}
