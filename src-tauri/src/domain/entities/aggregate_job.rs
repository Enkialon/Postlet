//! 会话聚合任务

use crate::domain::value_objects::{
    AccountId, AggregateJobStatus, AggregationOutcome, MessageId, TimestampMs,
};

/// 聚合任务（失败恢复 / 租约）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionAggregateJob {
    pub account_id: AccountId,
    pub message_id: MessageId,
    /// 每次有效消息变更重新入队时单调递增
    pub generation: i64,
    pub outcome: Option<AggregationOutcome>,
    pub status: AggregateJobStatus,
    pub attempts: i32,
    pub last_error: Option<String>,
    /// 最早可处理时间
    pub available_at: TimestampMs,
    pub updated_at: TimestampMs,
    /// processing 时非空
    pub lease_token: Option<String>,
    pub lease_expires_at: Option<TimestampMs>,
}
