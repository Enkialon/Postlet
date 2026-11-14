//! 在单次 spawn_blocking 中完成登录 + 多文件夹同步。

use super::client::{
    discover_folders, list_capabilities, session_login_blocking, ImapAuth, ImapEndpoint,
};
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

    let folder_names: Vec<String> = if requests.is_empty() {
        let discovered = discover_folders(&mut session)?;
        if discovered.iter().any(|n| n.eq_ignore_ascii_case("INBOX")) {
            vec!["INBOX".into()]
        } else {
            discovered.into_iter().take(1).collect()
        }
    } else {
        requests.iter().map(|r| r.folder.clone()).collect()
    };

    let mut folders = Vec::new();
    for name in folder_names {
        let req = requests.iter().find(|r| r.folder == name);
        let result = sync_folder(
            &mut session,
            &name,
            req.and_then(|r| r.stored_delta.as_deref()),
            req.and_then(|r| r.stored_next.as_deref()),
            req.map(|r| r.known_uids.as_slice()).unwrap_or(&[]),
            page_limit,
        )?;
        folders.push(result);
    }
    Ok(ImapSyncBatch {
        idle_supported,
        folders,
    })
}
