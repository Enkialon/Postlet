use crate::domain::value_objects::{AccountId, MessageId};

/// 聚合任务租约声明
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregateClaim {
    pub account_id: AccountId,
    pub message_id: MessageId,
    pub generation: i64,
    pub lease_token: String,
}

