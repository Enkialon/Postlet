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
    /// 是否包含默认排除的文件夹范围（Trash/Junk/Drafts/All）
    #[serde(default)]
    pub include_excluded_folders: bool,
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

impl From<crate::infrastructure::repository::sessions::SessionProjection> for SessionListItem {
    fn from(row: crate::infrastructure::repository::sessions::SessionProjection) -> Self {
        let display_name = crate::domain::services::session_display_name(
            row.session.kind,
            row.contact_display_name.as_deref(),
            row.session.normalized_address.as_ref().map(|a| a.as_str()),
            row.group_name.as_deref(),
        );
        let last_message_preview = row.last_message.map(|m| MessagePreview {
            id: m.id,
            subject: m.subject,
            body_preview: m.body_preview,
            from_address: m.from_address,
            from_name: m.from_name,
            received_at: m.received_at,
            sent_at: m.sent_at,
            is_read: m.is_read,
            is_outbound: m.is_outbound,
            has_attachments: m.has_attachments,
            importance: m.importance,
        });
        Self {
            session: row.session,
            display_name,
            last_message_preview,
            contact_display_name: row.contact_display_name,
            group_name: row.group_name,
        }
    }
}
