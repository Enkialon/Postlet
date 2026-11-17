//! 邮件文件夹同步状态

use crate::domain::value_objects::{AccountId, FolderId, ReceiveProtocol, SyncStatus, TimestampMs};

/// 按账户 + 文件夹保存的目录元数据与增量游标（非主导航）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MailFolderSyncState {
    pub account_id: AccountId,
    pub folder_id: FolderId,
    pub protocol: ReceiveProtocol,
    pub remote_binding: String,
    pub binding_revision: i64,
    pub is_retired: bool,
    /// 父文件夹；根为 None（多级树）
    pub parent_folder_id: Option<FolderId>,
    pub display_name: String,
    /// 是否纳入账户全局同步
    pub sync_enabled: bool,
    /// Adapter 提供的不透明远端代次；与同步进度分开。
    pub generation: Option<String>,
    /// 最近完成轮次的增量游标
    pub sync_cursor: Option<String>,
    /// 当前未完成轮次的分页续传位置
    pub page_cursor: Option<String>,
    /// 该文件夹最近一次增量同步成功完成时间
    pub synced_at: Option<TimestampMs>,
    pub sync_status: SyncStatus,
    pub last_error: Option<String>,
    pub updated_at: TimestampMs,
}
