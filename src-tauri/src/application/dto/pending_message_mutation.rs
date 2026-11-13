use crate::domain::value_objects::{AccountId, MessageId, TimestampMs};

/// 待推送的本地字段变更
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingMessageMutation {
    pub account_id: AccountId,
    pub message_id: MessageId,
    /// V1 仅支持 is_read；将来按字段扩展
    pub desired_is_read: bool,
    pub version: i64,
    pub attempts: i32,
    pub available_at: TimestampMs,
    pub last_error: Option<String>,
}

