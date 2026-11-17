//! 发送任务：阶段、尝试与恢复规则的纯领域模型。

use crate::domain::mail::{SendOutcome, SendStage};
use crate::domain::value_objects::{AccountId, MessageId, TimestampMs};

/// 发送任务状态机。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SendState {
    /// 已入队，尚未开始任何网络调用
    Queued,
    /// 已持久化 attempt_id，正在或曾经进入网络阶段
    Sending,
    /// 服务器明确接受（不代表最终送达）
    Accepted,
    /// 可退避重试的暂时失败
    Retryable,
    /// 确定失败，不再自动重试
    Failed,
    /// 结果不确定：可能已被接受，禁止自动重发
    Uncertain,
    /// 用户取消
    Cancelled,
}

impl SendState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Sending => "sending",
            Self::Accepted => "accepted",
            Self::Retryable => "retryable",
            Self::Failed => "failed",
            Self::Uncertain => "uncertain",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "queued" => Self::Queued,
            "sending" => Self::Sending,
            "accepted" => Self::Accepted,
            "retryable" => Self::Retryable,
            "failed" => Self::Failed,
            "uncertain" => Self::Uncertain,
            "cancelled" => Self::Cancelled,
            _ => return None,
        })
    }

    /// 是否处于终态（不再自动推进）
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Accepted | Self::Failed | Self::Uncertain | Self::Cancelled)
    }

    /// UI 是否应展示为「已发送」
    pub fn shows_as_sent(self) -> bool {
        matches!(self, Self::Accepted)
    }
}

/// 一次发送尝试
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendTask {
    pub task_id: String,
    pub account_id: AccountId,
    pub message_id: MessageId,
    pub content_version: i64,
    pub snapshot_id: String,
    pub state: SendState,
    pub attempt_id: Option<String>,
    pub claim_token: Option<String>,
    pub lease_until: Option<TimestampMs>,
    pub attempts: i32,
    pub next_attempt_at: Option<TimestampMs>,
    pub last_error_code: Option<String>,
    pub last_error: Option<String>,
    pub accepted_at: Option<TimestampMs>,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}

/// 退避：指数增长，上限 15 分钟（与 mutation 一致的重试节奏）
pub fn backoff_ms(attempts: i32) -> i64 {
    let exp = attempts.clamp(0, 6) as u32;
    (5_000i64)
        .saturating_mul(1i64 << exp)
        .min(15 * 60_000)
}

/// 由一次尝试结果决定下一个状态。
///
/// - Accepted：服务器明确接受
/// - Transient：仅当可证明内容未被接受时才允许自动重试
/// - Uncertain：可能已被接受，或阶段无法证明
pub fn next_state(result: SendOutcome, stage: SendStage) -> SendState {
    match result {
        SendOutcome::Accepted => SendState::Accepted,
        SendOutcome::AuthRejected | SendOutcome::Permanent => SendState::Failed,
        SendOutcome::Transient => {
            // DATA 已开始后即使收到暂时失败，也不能断言未被接受
            if stage == SendStage::Data {
                SendState::Uncertain
            } else {
                SendState::Retryable
            }
        }
        SendOutcome::Uncertain => SendState::Uncertain,
    }
}

/// 进程重启时如何处理「sending」：
/// 有接受证据则 accepted；否则无法证明未被接受 → uncertain，lease 到期也不自动重发。
pub fn recover_state(had_accepted_at: bool) -> SendState {
    if had_accepted_at {
        SendState::Accepted
    } else {
        SendState::Uncertain
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_after_data_is_uncertain_not_retryable() {
        assert_eq!(
            next_state(SendOutcome::Transient, SendStage::Envelope),
            SendState::Retryable
        );
        assert_eq!(
            next_state(SendOutcome::Transient, SendStage::Data),
            SendState::Uncertain
        );
    }

    #[test]
    fn auth_and_permanent_fail_without_retry() {
        assert_eq!(
            next_state(SendOutcome::AuthRejected, SendStage::Auth),
            SendState::Failed
        );
        assert_eq!(
            next_state(SendOutcome::Permanent, SendStage::Data),
            SendState::Failed
        );
    }

    #[test]
    fn only_accepted_shows_as_sent() {
        assert!(SendState::Accepted.shows_as_sent());
        for s in [
            SendState::Queued,
            SendState::Sending,
            SendState::Retryable,
            SendState::Failed,
            SendState::Uncertain,
            SendState::Cancelled,
        ] {
            assert!(!s.shows_as_sent(), "{s:?} must not display as sent");
        }
    }

    #[test]
    fn restart_without_acceptance_is_uncertain() {
        assert_eq!(recover_state(true), SendState::Accepted);
        assert_eq!(recover_state(false), SendState::Uncertain);
    }
}
