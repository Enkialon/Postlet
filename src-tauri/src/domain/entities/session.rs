//! 会话一等公民：私聊 / 群聊共用一张表

use crate::domain::value_objects::{
    AccountId, ContactId, EmailAddress, GroupId, MessageId, SessionId, SessionKind, TimestampMs,
};

/// 会话；联系人 / 群组是其关联投影，消息只通过 session_id 归属
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Session {
    pub id: SessionId,
    pub account_id: AccountId,
    pub kind: SessionKind,
    /// kind=private 时非空
    pub normalized_address: Option<EmailAddress>,
    /// 可空；删联系人时清空引用，保留会话
    pub contact_id: Option<ContactId>,
    /// kind=group 时非空
    pub group_id: Option<GroupId>,
    /// 被合并时指向存活根会话
    pub merged_into_id: Option<SessionId>,
    pub last_message_at: Option<TimestampMs>,
    pub last_message_id: Option<MessageId>,
    pub unread_count: i64,
    /// 本地隐藏时刻；非空则不进列表，有更新邮件时清除
    pub hidden_at: Option<TimestampMs>,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}
