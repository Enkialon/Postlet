//! IMAP 收信适配：连接、文件夹发现、UID 增量、FLAGS、删除检测、IDLE。

mod client;
mod idle;
mod parse;
mod session_ops;
mod sync;

pub use client::{
    discover_folder_tree, discover_folders, list_capabilities, session_login,
    session_login_blocking, uid_set_seen, uid_set_seen_async, ImapAuth, ImapEndpoint,
    ImapFolderNode,
};
pub use idle::{run_idle_until_cancel, IdleOutcome};
pub use session_ops::{sync_account_blocking, FolderSyncRequest, ImapSyncBatch};
pub use sync::{sync_folder, FolderSyncResult};
