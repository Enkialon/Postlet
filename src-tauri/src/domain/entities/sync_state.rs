//! 邮件文件夹同步状态

use crate::domain::value_objects::{AccountId, FolderId, SyncStatus, TimestampMs};

/// 按账户 + 文件夹保存的邮件增量游标（非文件夹导航模型）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MailFolderSyncState {
    pub account_id: AccountId,
    pub folder_id: FolderId,
    /// 最近完成轮次的增量游标
    pub delta_link: Option<String>,
    /// 当前未完成轮次的分页续传位置
    pub next_link: Option<String>,
    pub synced_at: Option<TimestampMs>,
    pub sync_status: SyncStatus,
    pub last_error: Option<String>,
    pub updated_at: TimestampMs,
}
