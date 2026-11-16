//! 远端快照模型（不含本地身份及投影状态）

use crate::domain::value_objects::{
    BodyContentType, EmailAddress, FolderId, GraphImmutableId, Importance, RecipientRole,
    TimestampMs,
};

/// Delta 未提供的字段：区分「未返回」与「明确清空」
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteField<T> {
    Unchanged,
    Value(T),
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
    pub graph_id: GraphImmutableId,
    pub graph_change_key: Option<String>,
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
    pub created_at_graph: Option<TimestampMs>,
    pub is_read: bool,
    pub is_draft: bool,
    pub has_attachments: bool,
    pub importance: Importance,
    pub folder_id: Option<FolderId>,
    pub parent_folder_name: Option<String>,
    pub remote_updated_at: Option<TimestampMs>,
}

/// Graph 邮件变更
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphMessageChange {
    /// 远端字段快照；本地 ID、同步状态与投影由用例决定
    Upsert(RemoteMessage),
    /// 文件夹移出 / 删除信号；须结合当前位置判定
    Remove { graph_id: GraphImmutableId },
}

/// Delta 分页结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailDeltaPage {
    pub messages: Vec<GraphMessageChange>,
    pub next_link: Option<String>,
    pub delta_link: Option<String>,
}

/// 按 ImmutableId 查询当前位置
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteMessageLocation {
    Present { folder_id: FolderId },
    NotFound,
}

/// 远端联系人快照
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteContact {
    pub graph_id: String,
    pub display_name: String,
    pub normalized_address: EmailAddress,
}

/// 联系人 Delta 分页
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactDeltaPage {
    pub upserts: Vec<RemoteContact>,
    pub removed_graph_ids: Vec<String>,
    pub delta_link: Option<String>,
    pub next_link: Option<String>,
}

/// 远端账户身份（不含本地同步与显示配置）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteAccount {
    /// 与本地 AccountId 绑定后的标识（由 Auth 基础设施映射）
    pub account_id: String,
    pub email: EmailAddress,
    pub display_name: String,
    pub aliases: Vec<EmailAddress>,
    pub tenant_id: Option<String>,
    pub home_account_id: Option<String>,
}

/// 带 RemoteField 的部分更新辅助（Delta 稀疏字段场景）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteMessagePatch {
    pub graph_id: GraphImmutableId,
    pub subject: RemoteField<String>,
    pub body_preview: RemoteField<String>,
    pub is_read: RemoteField<bool>,
    pub folder_id: RemoteField<Option<FolderId>>,
}
