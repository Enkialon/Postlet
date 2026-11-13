//! 联系人实体

use crate::domain::value_objects::{AccountId, ContactId, EmailAddress, TimestampMs};

/// 通讯录本地缓存
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contact {
    pub id: ContactId,
    pub account_id: AccountId,
    /// 可空：允许仅按地址建的占位联系人
    pub graph_id: Option<String>,
    pub display_name: String,
    /// 主地址（账户内唯一）
    pub normalized_address: EmailAddress,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}
