//! 邮件账户实体

use crate::domain::value_objects::{AccountId, EmailAddress, SyncStatus, TimestampMs};

/// 邮件账户（租户根；协议由 AccountMailTransport 配置）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
    /// 最近一次发起邮件同步的时间（调用/触发时刻，非成功时刻）
    pub mail_sync_triggered_at: Option<TimestampMs>,
    /// 最近一次联系人同步成功结束时间
    pub contact_synced_at: Option<TimestampMs>,
    pub mail_sync_status: SyncStatus,
    /// 最近一次失败原因；成功时清空
    pub mail_sync_error: Option<String>,
    /// 是否纳入「全部账号」会话/联系人/群组查询（默认是；与是否已登录无关）
    pub include_in_all: bool,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}
