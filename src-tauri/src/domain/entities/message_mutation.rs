use crate::domain::value_objects::{AccountId, FolderId, RemoteKey, MessageId, TimestampMs};

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
    /// 终止失败保留错误记录，但不再覆盖远端观测或自动派发。
    pub is_active: bool,
    pub target_folder_id: FolderId,
    pub target_remote_key: RemoteKey,
    pub target_location_revision: i64,
    pub target_binding_revision: i64,
    /// kind=read 时有值
    pub desired_is_read: Option<bool>,
    pub version: i64,
    pub attempts: i32,
    pub available_at: TimestampMs,
    pub last_error: Option<String>,
}
