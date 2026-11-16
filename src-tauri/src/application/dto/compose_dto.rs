//! 写信草稿 / 发送命令 DTO（与前端 domain/dto 同形）

use crate::domain::value_objects::{AccountId, EmailAddress, MessageId, SessionId, TimestampMs};

/// 写信附件草稿（仅元数据）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ComposeAttachmentDraft {
    pub id: String,
    pub name: String,
    pub size: i64,
    pub mime: String,
}

/// 写信收件人（标签展示名 + 地址）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ComposeRecipientRef {
    pub address: EmailAddress,
    pub display_name: String,
}

/// 会话级写信草稿
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ComposeDraft {
    pub account_id: AccountId,
    pub session_id: SessionId,
    /// 显式回复目标；None = 新邮件
    pub reply_to_message_id: Option<MessageId>,
    pub to: Vec<ComposeRecipientRef>,
    pub cc: Vec<ComposeRecipientRef>,
    pub bcc: Vec<ComposeRecipientRef>,
    pub subject: String,
    pub body_html: String,
    pub attachments: Vec<ComposeAttachmentDraft>,
    pub updated_at: TimestampMs,
}

/// 保存写信草稿
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SaveComposeDraftCommand {
    pub account_id: AccountId,
    pub session_id: SessionId,
    pub reply_to_message_id: Option<MessageId>,
    pub to: Vec<ComposeRecipientRef>,
    pub cc: Vec<ComposeRecipientRef>,
    pub bcc: Vec<ComposeRecipientRef>,
    pub subject: String,
    pub body_html: String,
    pub attachments: Vec<ComposeAttachmentDraft>,
}

/// 发送写信内容
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SendComposedMailCommand {
    pub account_id: AccountId,
    pub session_id: SessionId,
    pub reply_to_message_id: Option<MessageId>,
    pub to: Vec<ComposeRecipientRef>,
    pub cc: Vec<ComposeRecipientRef>,
    pub bcc: Vec<ComposeRecipientRef>,
    pub subject: String,
    pub body_html: String,
    pub attachments: Vec<ComposeAttachmentDraft>,
}
