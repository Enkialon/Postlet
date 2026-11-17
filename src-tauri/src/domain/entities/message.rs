//! 邮件与收件人实体

use crate::domain::value_objects::{
    AccountId, BodyContentType, EmailAddress, FolderId, RemoteKey, Importance, MessageId,
    MessageSyncStatus, RecipientRole, SessionId, TimestampMs,
};

/// 邮件真相；归属唯一 Session（session_id）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub account_id: AccountId,
    pub session_id: SessionId,
    /// 当前位置的不透明远端键；本地草稿可空
    pub remote_key: Option<RemoteKey>,
    /// RFC Message-ID
    pub internet_message_id: Option<String>,
    /// RFC In-Reply-To（用于找父信进同一 Session）
    pub in_reply_to: Option<String>,
    pub subject: String,
    pub body_preview: String,
    pub body_content_type: BodyContentType,
    /// None=未加载；Some(空串)=已加载且正文为空
    pub body_content: Option<String>,
    pub from_address: Option<EmailAddress>,
    pub from_name: String,
    pub received_at: Option<TimestampMs>,
    pub sent_at: Option<TimestampMs>,
    pub is_read: bool,
    pub is_draft: bool,
    pub remote_is_read: Option<bool>,
    pub remote_is_draft: Option<bool>,
    pub remote_is_deleted: Option<bool>,
    /// 是否本账户发出（sync 落库缓存；读路径按当前 self 集合现算覆盖）
    pub is_outbound: bool,
    pub has_attachments: bool,
    pub importance: Importance,
    /// 当前位置文件夹；非主导航
    pub folder_id: Option<FolderId>,
    pub parent_folder_name: Option<String>,
    pub sync_status: MessageSyncStatus,
    pub local_updated_at: TimestampMs,
    pub remote_updated_at: Option<TimestampMs>,
}

/// 收件人拆行
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MessageRecipient {
    pub account_id: AccountId,
    pub message_id: MessageId,
    pub role: RecipientRole,
    /// 同角色内顺序
    pub seq: i32,
    pub normalized_address: EmailAddress,
    pub display_name: String,
}

/// 邮件 + 收件人读写聚合
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MessageWithRecipients {
    pub message: Message,
    pub recipients: Vec<MessageRecipient>,
}
