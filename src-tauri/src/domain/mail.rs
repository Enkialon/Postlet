//! 协议中性的远端邮件事实；无 IO。

use crate::domain::value_objects::{BodyContentType, EmailAddress, FolderId, RemoteKey, Importance, RecipientRole, TimestampMs};

/// 远端收件人（无本地外键）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRecipient {
    pub role: RecipientRole,
    pub seq: i32,
    pub normalized_address: EmailAddress,
    pub display_name: String,
}

/// 远端邮件快照
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteMessage {
    pub remote_key: RemoteKey,
    pub internet_message_id: Option<String>,
    /// RFC In-Reply-To（若协议可得）
    pub in_reply_to: Option<String>,
    pub subject: String,
    pub body_preview: String,
    pub body_content_type: BodyContentType,
    /// 可空正文；未拉取时为 None
    pub body_content: Option<String>,
    pub from_address: Option<EmailAddress>,
    pub from_name: String,
    pub recipients: Vec<RemoteRecipient>,
    pub received_at: Option<TimestampMs>,
    pub sent_at: Option<TimestampMs>,
    pub is_read: bool,
    pub is_draft: bool,
    pub has_attachments: bool,
    pub importance: Importance,
    pub folder_id: Option<FolderId>,
    pub parent_folder_name: Option<String>,
    pub remote_updated_at: Option<TimestampMs>,
}

/// 统一位置邮件变更
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteMessageChange {
    /// 远端字段快照；本地 ID、同步状态与投影由用例决定
    Upsert(RemoteMessage),
    /// 文件夹移出 / 删除信号；须结合当前位置判定
    Remove { remote_key: RemoteKey },
}
