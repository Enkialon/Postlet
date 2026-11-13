//! 邮件与收件人实体

use crate::domain::value_objects::{
    AccountId, BodyContentType, EmailAddress, FolderId, GraphImmutableId, Importance, MessageId,
    MessageSyncStatus, RecipientRole, ThreadId, TimestampMs,
};

/// 邮件真相（Graph 同步落库）；会话归属不改写本实体
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub account_id: AccountId,
    /// Graph ImmutableId；本地草稿可空
    pub graph_id: Option<GraphImmutableId>,
    /// Graph conversationId；联系人投影不要求非空，线程归组要求非空
    pub thread_id: Option<ThreadId>,
    pub graph_change_key: Option<String>,
    /// RFC Message-ID
    pub internet_message_id: Option<String>,
    pub conversation_index: Option<Vec<u8>>,
    pub subject: String,
    pub body_preview: String,
    pub body_content_type: BodyContentType,
    /// None=未加载；Some(空串)=已加载且正文为空
    pub body_content: Option<String>,
    pub from_address: Option<EmailAddress>,
    pub from_name: String,
    pub received_at: Option<TimestampMs>,
    pub sent_at: Option<TimestampMs>,
    pub created_at_graph: Option<TimestampMs>,
    pub is_read: bool,
    pub is_draft: bool,
    /// 是否本账户发出（结合 self 集合判定后落库）
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
