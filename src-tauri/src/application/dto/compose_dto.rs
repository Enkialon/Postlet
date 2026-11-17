//! 写信草稿 / 发送命令 DTO（与前端 domain/dto 同形）

use crate::domain::value_objects::{AccountId, EmailAddress, MessageId, SessionId, TimestampMs};

/// 写信附件草稿（稳定 content_id 引用受管理内容）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ComposeAttachmentDraft {
    pub content_id: String,
    pub name: String,
    pub size: i64,
    pub mime: String,
}

/// 导入附件到受管理存储并挂到会话草稿
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ImportAttachmentCommand {
    pub account_id: AccountId,
    pub session_id: SessionId,
    pub name: String,
    pub mime: String,
    /// 声明字节数，须与解码后长度一致
    pub byte_length: i64,
    /// 标准 Base64 编码的文件内容
    pub content_base64: String,
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

/// 发送结果：消息 + 队列状态（入队不等于已送达）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ComposeSendResult {
    pub message: crate::domain::entities::MessageWithRecipients,
    pub send_state: crate::domain::entities::SendState,
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
