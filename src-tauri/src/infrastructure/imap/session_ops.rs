//! 在单次 spawn_blocking 中完成登录 + 多文件夹同步。

use super::client::{list_capabilities, session_login_blocking, ImapAuth, ImapEndpoint};
use super::sync::{sync_folder, FolderSyncResult};
use crate::application::error::PortError;

#[derive(Debug, Clone)]
pub struct ImapSyncBatch {
    pub idle_supported: bool,
    pub folders: Vec<FolderSyncResult>,
}

#[derive(Debug, Clone)]
pub struct FolderSyncRequest {
    pub folder: String,
    pub stored_delta: Option<String>,
    pub stored_next: Option<String>,
    pub known_uids: Vec<(u32, bool)>,
}

pub fn sync_account_blocking(
    endpoint: &ImapEndpoint,
    auth: &ImapAuth,
    requests: &[FolderSyncRequest],
    page_limit: u32,
) -> Result<ImapSyncBatch, PortError> {
    let mut session = session_login_blocking(endpoint, auth)?;
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
        let result = sync_folder(
            &mut session,
            &req.folder,
            req.stored_delta.as_deref(),
            req.stored_next.as_deref(),
            &req.known_uids,
            page_limit,
        )?;
        folders.push(result);
    }
    let _ = session.logout();
    Ok(ImapSyncBatch {
        idle_supported,
        folders,
    })
}
