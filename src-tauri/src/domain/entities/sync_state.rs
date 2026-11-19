//! 邮件文件夹同步状态

use crate::domain::services::{FolderRole, RoleSource};
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
    /// IMAP SPECIAL-USE 对齐角色
    pub role: FolderRole,
    /// 角色来源：服务器 > 手动 > 名称猜测
    pub role_source: RoleSource,
    /// 用户三态覆盖：None=自动识别，Some(true)=是垃圾夹，Some(false)=不是垃圾夹
    pub junk_override: Option<bool>,
    /// 生效角色：由 `role` 与 `junk_override` 派生，不单独落库
    pub effective_role: FolderRole,
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
