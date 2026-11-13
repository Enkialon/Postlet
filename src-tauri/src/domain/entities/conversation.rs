//! 会话列表缓存与消息附着

use crate::domain::value_objects::{
    AccountId, ContactId, ConversationId, ConversationKind, EmailAddress, GroupId, MessageId,
    TimestampMs,
};

/// 会话列表缓存；归属真相在 group_thread + conversation_messages
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Conversation {
    pub id: ConversationId,
    pub account_id: AccountId,
    pub kind: ConversationKind,
    /// kind=contact 时非空
    pub normalized_address: Option<EmailAddress>,
    /// 可空；删联系人时清空引用，保留会话
    pub contact_id: Option<ContactId>,
    /// kind=group 时非空
    pub group_id: Option<GroupId>,
    pub last_message_at: Option<TimestampMs>,
    /// 须指向当前附着的非软删消息
    pub last_message_id: Option<MessageId>,
    /// 仅统计当前会话内非软删且未读
    pub unread_count: i64,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}

/// 消息 ↔ 会话多对多附着
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ConversationMessageLink {
    pub account_id: AccountId,
    pub conversation_id: ConversationId,
    pub message_id: MessageId,
    pub attached_at: TimestampMs,
}
