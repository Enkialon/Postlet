//! 邮件与收件人实体

use crate::domain::value_objects::{
    AccountId, BodyContentType, EmailAddress, FolderId, RemoteKey, Importance, MessageId,
    RecipientRole, SessionId, TimestampMs,
};

/// 邮件真相；归属唯一 Session（session_id）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub account_id: AccountId,
    pub session_id: SessionId,
    /// 当前位置的不透明远端键；本地草稿可空
    pub remote_key: Option<RemoteKey>,
    pub location_status: Option<crate::domain::mail::LocationStatus>,
    pub location_revision: i64,
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
    /// 读模型投影：发送队列状态（None 表示不是本次本地发送）
    #[serde(default)]
    pub send_state: Option<crate::domain::entities::SendState>,
    /// 读模型投影：本地删除意图待远端确认（来自 mutation）
    #[serde(default)]
    pub pending_delete: bool,
    /// 读模型投影：消息所在文件夹的 IMAP 对齐角色
    #[serde(default)]
    pub folder_role: Option<crate::domain::services::FolderRole>,
    /// 读模型投影：待确认的垃圾移动方向（None=无）
    #[serde(default)]
    pub pending_junk_move: Option<crate::domain::entities::JunkMoveDirection>,
}
