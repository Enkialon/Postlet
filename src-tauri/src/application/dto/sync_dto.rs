//! 同步相关 DTO

use crate::domain::entities::MailFolderSyncState;
use crate::domain::value_objects::{AccountId, FolderId, SyncStatus, TimestampMs};

/// 邮件同步进度读模型
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailSyncProgress {
    pub account_id: AccountId,
    pub mail_sync_status: SyncStatus,
    pub mail_sync_error: Option<String>,
    pub mail_synced_at: Option<TimestampMs>,
    pub folders: Vec<MailFolderSyncState>,
}

/// 联系人同步进度读模型
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactSyncProgress {
    pub account_id: AccountId,
    pub contact_synced_at: Option<TimestampMs>,
    pub contact_delta_link: Option<String>,
}

/// 触发邮件同步
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerMailSyncCommand {
    pub account_id: AccountId,
    /// 空则使用当前跟踪文件夹集合
    pub folder_ids: Option<Vec<FolderId>>,
}

/// 触发联系人同步
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerContactSyncCommand {
    pub account_id: AccountId,
}
