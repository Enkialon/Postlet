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
    /// 认领令牌：结果确认必须匹配
    pub claim_token: Option<String>,
    /// 认领时的意图版本
    pub claimed_version: Option<i64>,
    /// 认领租约；到期只表示需要恢复核对，不代表可以并行执行
    pub lease_until: Option<TimestampMs>,
    /// 执行阶段：started（已开始网络写）/ sent（已发送待确认）
    pub execution_phase: Option<MutationPhase>,
    pub last_error_code: Option<String>,
}

/// 一次远端写尝试的阶段
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationPhase {
    Started,
    Sent,
    Confirmed,
}

impl MutationPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Sent => "sent",
            Self::Confirmed => "confirmed",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "started" => Some(Self::Started),
            "sent" => Some(Self::Sent),
            "confirmed" => Some(Self::Confirmed),
            _ => None,
        }
    }
}
