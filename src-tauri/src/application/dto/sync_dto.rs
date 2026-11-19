//! 同步相关 DTO

use crate::domain::entities::MailFolderSyncState;
use crate::domain::value_objects::{AccountId, FolderId, SyncStatus, TimestampMs};

/// 邮件同步进度读模型
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MailSyncProgress {
    pub account_id: AccountId,
    pub mail_sync_status: SyncStatus,
    pub mail_sync_error: Option<String>,
    pub mail_sync_triggered_at: Option<TimestampMs>,
    pub folders: Vec<MailFolderSyncState>,
}

/// 联系人同步进度读模型
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ContactSyncProgress {
    pub account_id: AccountId,
    pub contact_synced_at: Option<TimestampMs>,
    pub contact_delta_link: Option<String>,
}

/// 触发邮件同步
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TriggerMailSyncCommand {
    pub account_id: AccountId,
    /// 空则使用当前跟踪文件夹集合
    pub folder_ids: Option<Vec<FolderId>>,
}

/// 触发联系人同步
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TriggerContactSyncCommand {
    pub account_id: AccountId,
}

/// 设置文件夹是否纳入全局同步
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SetFolderSyncEnabledCommand {
    pub account_id: AccountId,
    pub folder_id: FolderId,
    pub sync_enabled: bool,
}

/// 设置文件夹垃圾邮件角色覆盖（三态：null=自动识别，true=是，false=不是）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SetFolderJunkOverrideCommand {
    pub account_id: AccountId,
    pub folder_id: FolderId,
    pub junk_override: Option<bool>,
}
