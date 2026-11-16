//! 联系人实体

use crate::domain::value_objects::{
    AccountId, ContactId, ContactSource, EmailAddress, TimestampMs,
};

/// 人档案：真联系人（graph/manual）与观测档（observed）同表
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Contact {
    pub id: ContactId,
    pub account_id: AccountId,
    /// Graph 通讯录远端 id；observed / 纯手动可空
    pub graph_id: Option<String>,
    pub display_name: String,
    /// 主地址（账户内唯一）
    pub normalized_address: EmailAddress,
    pub source: ContactSource,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}
