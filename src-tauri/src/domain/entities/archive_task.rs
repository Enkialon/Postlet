//! 已发送副本归档任务：只处理归档，不重发邮件。

use crate::domain::value_objects::{AccountId, FolderId, MessageId, TimestampMs};

/// 账号 Sent 策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchivePolicy {
    /// 服务器发送时自动保存副本（Graph saveToSentItems）
    ServerSaved,
    /// 客户端 APPEND 到 Sent（SMTP 发信）
    ClientAppend,
    /// 策略未确认：不执行任何一种
    NeedsConfiguration,
}

impl ArchivePolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ServerSaved => "server_saved",
            Self::ClientAppend => "client_append",
            Self::NeedsConfiguration => "needs_configuration",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        Some(match raw {
            "server_saved" => Self::ServerSaved,
            "client_append" => Self::ClientAppend,
            "needs_configuration" => Self::NeedsConfiguration,
            _ => return None,
        })
    }

    /// 默认策略：Graph 服务器保存，IMAP/SMTP 客户端 APPEND
    pub fn default_for(receive_protocol: crate::domain::value_objects::ReceiveProtocol) -> Self {
        match receive_protocol {
            crate::domain::value_objects::ReceiveProtocol::Graph => Self::ServerSaved,
            crate::domain::value_objects::ReceiveProtocol::Imap => Self::ClientAppend,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveState {
    /// 等服务器副本出现（server_saved）
    WaitingServerCopy,
    /// 需要 APPEND（client_append）
    AppendPending,
    /// 已发出 APPEND，结果未确认
    Appending,
    /// 已确认归档
    Confirmed,
    /// 可重试失败
    Retryable,
    /// 结果不确定：禁止自动重复 APPEND
    Uncertain,
    /// 策略未配置
    NeedsConfiguration,
}

impl ArchiveState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WaitingServerCopy => "waiting_server_copy",
            Self::AppendPending => "append_pending",
            Self::Appending => "appending",
            Self::Confirmed => "confirmed",
            Self::Retryable => "retryable",
            Self::Uncertain => "uncertain",
            Self::NeedsConfiguration => "needs_configuration",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        Some(match raw {
            "waiting_server_copy" => Self::WaitingServerCopy,
            "append_pending" => Self::AppendPending,
            "appending" => Self::Appending,
            "confirmed" => Self::Confirmed,
            "retryable" => Self::Retryable,
            "uncertain" => Self::Uncertain,
            "needs_configuration" => Self::NeedsConfiguration,
            _ => return None,
        })
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Confirmed | Self::Uncertain | Self::NeedsConfiguration
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveTask {
    pub task_id: String,
    pub account_id: AccountId,
    pub send_task_id: String,
    pub snapshot_id: String,
    pub state: ArchiveState,
    pub target_folder_id: Option<FolderId>,
    pub target_binding_revision: Option<i64>,
    pub attempt_id: Option<String>,
    pub claim_token: Option<String>,
    pub lease_until: Option<TimestampMs>,
    pub next_attempt_at: Option<TimestampMs>,
    pub last_error: Option<String>,
    pub confirmed_message_id: Option<MessageId>,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}

/// 首次入队：按策略决定初始状态
pub fn initial_state(policy: ArchivePolicy) -> ArchiveState {
    match policy {
        ArchivePolicy::ServerSaved => ArchiveState::WaitingServerCopy,
        ArchivePolicy::ClientAppend => ArchiveState::AppendPending,
        ArchivePolicy::NeedsConfiguration => ArchiveState::NeedsConfiguration,
    }
}

/// APPEND 结果 → 下一状态：失败可重试，结果未知必须 uncertain
pub fn next_after_append(accepted: Option<bool>) -> ArchiveState {
    match accepted {
        Some(true) => ArchiveState::Confirmed,
        Some(false) => ArchiveState::Retryable,
        None => ArchiveState::Uncertain,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_append_result_is_uncertain_not_retryable() {
        assert_eq!(next_after_append(Some(true)), ArchiveState::Confirmed);
        assert_eq!(next_after_append(Some(false)), ArchiveState::Retryable);
        assert_eq!(next_after_append(None), ArchiveState::Uncertain);
    }

    #[test]
    fn default_policy_follows_protocol() {
        use crate::domain::value_objects::ReceiveProtocol;
        assert_eq!(
            ArchivePolicy::default_for(ReceiveProtocol::Graph),
            ArchivePolicy::ServerSaved
        );
        assert_eq!(
            ArchivePolicy::default_for(ReceiveProtocol::Imap),
            ArchivePolicy::ClientAppend
        );
        assert_eq!(
            initial_state(ArchivePolicy::NeedsConfiguration),
            ArchiveState::NeedsConfiguration
        );
    }
}
