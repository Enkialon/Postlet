//! 协议中性的远端邮件事实；无 IO。

use crate::domain::value_objects::{BodyContentType, EmailAddress, FolderId, RemoteKey, Importance, RecipientRole, TimestampMs};

/// None 表示没有观测到该标志；显式 false 才表示清除。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RemoteFlags {
    pub is_read: Option<bool>,
    pub is_draft: Option<bool>,
    pub is_deleted: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocationStatus { Present, Missing, Invalidated }

impl LocationStatus {
    pub fn as_str(self) -> &'static str {
        match self { Self::Present => "present", Self::Missing => "missing", Self::Invalidated => "invalidated" }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value { "present" => Some(Self::Present), "missing" => Some(Self::Missing), "invalidated" => Some(Self::Invalidated), _ => None }
    }
}

/// 一次完整目录发现返回的绑定事实；本地 FolderId 由仓储分配。
#[derive(Debug, Clone)]
pub struct DiscoveredFolder {
    pub remote_binding: String,
    pub parent_binding: Option<String>,
    pub display_name: String,
}

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
    pub flags: RemoteFlags,
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
    FlagsChanged { remote_key: RemoteKey, flags: RemoteFlags },
    /// 文件夹移出 / 删除信号；须结合当前位置判定
    Remove { remote_key: RemoteKey },
}
