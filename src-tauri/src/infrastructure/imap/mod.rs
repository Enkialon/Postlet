//! IMAP 收信适配：连接、文件夹发现、UID 增量、FLAGS、删除检测、IDLE。

mod client;
mod connection;
mod idle;
mod parse;
pub use parse::remote_id as imap_remote_id;
mod session_ops;
mod sync;
mod stream;

pub use client::{
    discover_folder_tree, discover_folders, list_capabilities, load_connection,
    append_message_async, uid_delete, uid_delete_async, uid_move_async, uid_set_seen,
    uid_set_seen_async,
    ImapAuth, ImapEndpoint, ImapFolderNode,
};
pub use connection::session_login;
pub use idle::{run_idle_until_cancel, IdleOutcome};
pub use session_ops::{sync_account_blocking, ImapSyncBatch};
pub use sync::{FolderSyncRequest, FolderSyncResult};
