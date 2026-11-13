//! 会话列表查询与读模型

use crate::domain::entities::Conversation;
use crate::domain::value_objects::{
    AccountId, ConversationId, ConversationKind, EmailAddress, Importance, MessageId,
    PageDirection, TimestampMs,
};

/// 会话列表查询
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ConversationListQuery {
    pub account_id: AccountId,
    pub cursor: Option<String>,
    pub limit: u32,
    /// 可选：仅 contact / 仅 group
    pub kind: Option<ConversationKind>,
}

/// 会话内消息查询
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ConversationMessagesQuery {
    pub account_id: AccountId,
    pub conversation_id: ConversationId,
    pub cursor: Option<String>,
    pub limit: u32,
    /// 默认 Older
    pub direction: PageDirection,
}

/// 列表用消息摘要
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MessagePreview {
    pub id: MessageId,
    pub subject: String,
    pub body_preview: String,
    pub from_address: Option<EmailAddress>,
    pub from_name: String,
    pub received_at: Option<TimestampMs>,
    pub sent_at: Option<TimestampMs>,
    pub is_read: bool,
    pub is_outbound: bool,
    pub has_attachments: bool,
    pub importance: Importance,
}

/// 会话列表项（含预览派生）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ConversationListItem {
    pub conversation: Conversation,
    /// 列表预览；软删消息不出现
    pub last_message_preview: Option<MessagePreview>,
    pub contact_display_name: Option<String>,
    pub group_name: Option<String>,
}
