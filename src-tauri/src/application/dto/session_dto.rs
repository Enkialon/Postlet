//! 会话列表查询与读模型

use crate::domain::entities::Session;
use crate::domain::value_objects::{
    AccountId, EmailAddress, Importance, MessageId, PageDirection, SessionId, SessionKind,
    TimestampMs,
};

/// 会话列表查询；`account_id` 为空时跨账号
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SessionListQuery {
    pub account_id: Option<AccountId>,
    pub cursor: Option<String>,
    pub limit: u32,
    /// 可选：仅 private / 仅 group
    pub kind: Option<SessionKind>,
}

/// 会话内消息查询
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessagesQuery {
    pub account_id: AccountId,
    pub session_id: SessionId,
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

/// 会话列表项（含预览与显示名）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SessionListItem {
    pub session: Session,
    /// 读时解析：私聊=联系人名或地址；群聊=群名
    pub display_name: String,
    /// 列表预览；软删消息不出现
    pub last_message_preview: Option<MessagePreview>,
    pub contact_display_name: Option<String>,
    pub group_name: Option<String>,
}
