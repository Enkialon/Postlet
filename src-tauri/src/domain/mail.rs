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
    /// 服务器声明的角色（SPECIAL-USE / well-known 已映射）；无则 None
    pub server_role: Option<crate::domain::services::FolderRole>,
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
    /// 可下载附件（正文 part 与 CID inline 不在此列）
    pub attachments: Vec<RemoteAttachment>,
    pub importance: Importance,
    pub folder_id: Option<FolderId>,
    pub parent_folder_name: Option<String>,
    pub remote_updated_at: Option<TimestampMs>,
}

/// 可下载附件内容（协议无关）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteAttachment {
    pub name: String,
    pub mime: String,
    pub bytes: Vec<u8>,
    pub inline: bool,
    pub content_id: Option<String>,
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

/// 发送所处阶段；用于区分「尚未被接受」与「可能已被接受」。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SendStage {
    /// 连接 / TLS 握手
    Connect,
    /// 认证
    Auth,
    /// MAIL FROM / RCPT TO
    Envelope,
    /// DATA 已开始发送，最终应答未确认
    Data,
}

/// 一次发送尝试的业务结论（协议中立，Graph 与 SMTP 共用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SendOutcome {
    /// 服务器已明确接受
    Accepted,
    /// 认证被拒；需用户修正配置
    AuthRejected,
    /// 永久拒绝（5xx / 内容或地址不可接受）；不应自动重试
    Permanent,
    /// 暂时拒绝（4xx / 连接失败且可证明尚未提交内容）；可退避重试
    Transient,
    /// 结果不确定（DATA 已发出但未拿到最终应答、进程中断等）；只能标记待确认
    Uncertain,
}

impl SendOutcome {
    /// 是否允许自动重试（仅当有证据表明未被接受）
    pub fn retryable(self) -> bool {
        matches!(self, Self::Transient)
    }
}

/// 发送结果：结论 + 发生阶段
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SendResult {
    pub outcome: SendOutcome,
    pub stage: SendStage,
}

impl SendResult {
    pub fn new(outcome: SendOutcome, stage: SendStage) -> Self {
        Self { outcome, stage }
    }
}
