//! Outlook 账户实体

use crate::domain::value_objects::{
    AccountId, EmailAddress, SyncStatus, TimestampMs,
};

/// Outlook 账户（租户根）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub id: AccountId,
    /// 主邮箱（规范化）
    pub email: EmailAddress,
    pub display_name: String,
    /// self 额外身份；与 email 组成排除集合
    pub aliases: Vec<EmailAddress>,
    pub tenant_id: Option<String>,
    pub home_account_id: Option<String>,
    /// 联系人 Delta 游标
    pub contact_delta_link: Option<String>,
    /// 最近一次账户邮件同步轮次全部成功结束时间
    pub mail_synced_at: Option<TimestampMs>,
    /// 最近一次联系人同步成功结束时间
    pub contact_synced_at: Option<TimestampMs>,
    pub mail_sync_status: SyncStatus,
    /// 最近一次失败原因；成功时清空
    pub mail_sync_error: Option<String>,
    /// 群组消息是否同时出现在联系人会话投影（默认否）
    pub show_group_messages_in_contact_conversation: bool,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}
