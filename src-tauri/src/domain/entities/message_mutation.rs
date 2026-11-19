use crate::domain::value_objects::{AccountId, FolderId, RemoteKey, MessageId, TimestampMs};

/// 待推送的本地字段变更种类
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingMutationKind {
    Read,
    /// 默认删除语义：移入 Trash（目标必须是已确认的 trash 角色）
    Trash,
    /// 用户明确选择的永久删除
    Delete,
    /// 用户手动标为垃圾：移入生效垃圾文件夹
    Junk,
    /// 用户手动移出垃圾：移入同账号生效的收件箱
    Unjunk,
}

impl PendingMutationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Trash => "trash",
            Self::Delete => "delete",
            Self::Junk => "junk",
            Self::Unjunk => "unjunk",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "read" => Some(Self::Read),
            "trash" => Some(Self::Trash),
            "delete" => Some(Self::Delete),
            "junk" => Some(Self::Junk),
            "unjunk" => Some(Self::Unjunk),
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
    /// kind=Trash / Junk / Unjunk 时的目标文件夹（源位置仍是 target_folder_id）
    pub destination_folder_id: Option<FolderId>,
}

/// 垃圾移动方向（读模型投影：提交后、确认前均保留）
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JunkMoveDirection {
    /// 标为垃圾邮件
    ToJunk,
    /// 移出垃圾邮件
    OutOfJunk,
}

impl JunkMoveDirection {
    pub fn kind(self) -> PendingMutationKind {
        match self {
            Self::ToJunk => PendingMutationKind::Junk,
            Self::OutOfJunk => PendingMutationKind::Unjunk,
        }
    }

    pub fn from_kind(kind: PendingMutationKind) -> Option<Self> {
        match kind {
            PendingMutationKind::Junk => Some(Self::ToJunk),
            PendingMutationKind::Unjunk => Some(Self::OutOfJunk),
            _ => None,
        }
    }

    /// 目标文件夹的生效角色是否仍符合该方向
    pub fn accepts(self, destination_role: crate::domain::services::FolderRole) -> bool {
        match self {
            Self::ToJunk => destination_role.is_junk(),
            Self::OutOfJunk => destination_role == crate::domain::services::FolderRole::Inbox,
        }
    }
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
