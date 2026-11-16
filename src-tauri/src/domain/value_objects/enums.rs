//! 领域枚举

/// Unix 毫秒时间戳
pub type TimestampMs = i64;

/// 账户级 / 文件夹级同步状态机
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncStatus {
    Idle,
    Running,
    Error,
}

impl SyncStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "running",
            Self::Error => "error",
        }
    }
}

/// 单封邮件相对远端的同步状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageSyncStatus {
    Synced,
    PendingLocal,
    DeletedRemote,
}

impl MessageSyncStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Synced => "synced",
            Self::PendingLocal => "pending_local",
            Self::DeletedRemote => "deleted_remote",
        }
    }
}

/// 邮件正文类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyContentType {
    Text,
    Html,
}

impl BodyContentType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Html => "html",
        }
    }
}

/// 重要度
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Importance {
    Low,
    Normal,
    High,
}

impl Importance {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Normal => "normal",
            Self::High => "high",
        }
    }
}

/// 收件人角色（对应 message_recipients.role）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipientRole {
    To,
    Cc,
    Bcc,
    ReplyTo,
}

impl RecipientRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::To => "to",
            Self::Cc => "cc",
            Self::Bcc => "bcc",
            Self::ReplyTo => "reply_to",
        }
    }
}

/// 群组诞生方式（不是线程归组的第二套规则）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupSource {
    Auto,
    Manual,
}

impl GroupSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Manual => "manual",
        }
    }
}

/// 联系人来源：真联系人 vs 邮件/群观测档（同表不同关系）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContactSource {
    /// Graph 通讯录同步
    Graph,
    /// 用户在本 App 手动保存
    Manual,
    /// 邮件/群观测到的短名档；不进联系人页 / 写信默认联想
    Observed,
}

impl ContactSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Graph => "graph",
            Self::Manual => "manual",
            Self::Observed => "observed",
        }
    }

    /// 是否出现在通讯录页 / 写信默认选人
    pub fn is_address_book(self) -> bool {
        matches!(self, Self::Graph | Self::Manual)
    }
}

/// 会话种类（一张 sessions 表内区分）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    Private,
    Group,
}

impl SessionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Private => "private",
            Self::Group => "group",
        }
    }
}

/// 会话聚合任务状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AggregateJobStatus {
    Pending,
    Processing,
    Completed,
    Retry,
    Failed,
}

impl AggregateJobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Processing => "processing",
            Self::Completed => "completed",
            Self::Retry => "retry",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "processing" => Some(Self::Processing),
            "completed" => Some(Self::Completed),
            "retry" => Some(Self::Retry),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

/// 会话内消息分页方向
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageDirection {
    Older,
    Newer,
}

/// 聚合结果终态 / 中间态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AggregationOutcome {
    Completed,
    Partial,
    Deferred,
    Skipped,
}

impl AggregationOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Partial => "partial",
            Self::Deferred => "deferred",
            Self::Skipped => "skipped",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "completed" => Some(Self::Completed),
            "partial" => Some(Self::Partial),
            "deferred" => Some(Self::Deferred),
            "skipped" => Some(Self::Skipped),
            _ => None,
        }
    }
}

/// 收信协议（按账号配置，非服务商品牌）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiveProtocol {
    Graph,
    Imap,
}

impl ReceiveProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Graph => "graph",
            Self::Imap => "imap",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "graph" => Some(Self::Graph),
            "imap" => Some(Self::Imap),
            _ => None,
        }
    }
}

/// 发信协议（按账号配置，非服务商品牌）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SendProtocol {
    Graph,
    Smtp,
}

impl SendProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Graph => "graph",
            Self::Smtp => "smtp",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "graph" => Some(Self::Graph),
            "smtp" => Some(Self::Smtp),
            _ => None,
        }
    }
}

/// 邮箱协议认证方式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailAuthKind {
    Password,
    OAuth2,
}

impl MailAuthKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Password => "password",
            Self::OAuth2 => "oauth2",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "password" => Some(Self::Password),
            "oauth2" => Some(Self::OAuth2),
            _ => None,
        }
    }
}

/// TLS 连接模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailTlsMode {
    /// 隐式 TLS（如 IMAPS 993 / SMTPS 465）
    Implicit,
    /// STARTTLS
    StartTls,
    /// 明文（仅测试）
    Plain,
}

impl MailTlsMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Implicit => "implicit",
            Self::StartTls => "starttls",
            Self::Plain => "plain",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "implicit" => Some(Self::Implicit),
            "starttls" => Some(Self::StartTls),
            "plain" => Some(Self::Plain),
            _ => None,
        }
    }
}
