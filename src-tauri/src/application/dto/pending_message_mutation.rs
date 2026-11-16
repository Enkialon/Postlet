use crate::domain::value_objects::{AccountId, MessageId, TimestampMs};

/// 待推送的本地字段变更种类
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingMutationKind {
    Read,
    Delete,
}

impl PendingMutationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Delete => "delete",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "read" => Some(Self::Read),
            "delete" => Some(Self::Delete),
            _ => None,
        }
    }
}

/// 待推送的本地字段变更
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingMessageMutation {
    pub account_id: AccountId,
    pub message_id: MessageId,
    pub kind: PendingMutationKind,
    /// kind=read 时有值
    pub desired_is_read: Option<bool>,
    pub version: i64,
    pub attempts: i32,
    pub available_at: TimestampMs,
    pub last_error: Option<String>,
}
