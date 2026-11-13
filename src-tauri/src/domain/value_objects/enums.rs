//! 领域枚举

/// Unix 毫秒时间戳
pub type TimestampMs = i64;

/// 账户级 / 文件夹级同步状态机
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SyncStatus {
    /// 空闲
    Idle,
    /// 进行中
    Running,
    /// 失败
    Error,
}

/// 单封邮件相对远端的同步状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MessageSyncStatus {
    /// 已与远端对齐
    Synced,
    /// 仅本地变更，待推送
    PendingLocal,
    /// 远端已删（软删）；不参与列表/未读
    DeletedRemote,
}

/// 邮件正文类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BodyContentType {
    Text,
    Html,
}

/// 重要度
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Importance {
    Low,
    Normal,
    High,
}

/// 收件人角色（对应 message_recipients.role）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecipientRole {
    To,
    Cc,
    Bcc,
    ReplyTo,
}

/// 群组诞生方式（不是线程归组的第二套规则）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GroupSource {
    /// 聚合自动创建
    Auto,
    /// 用户空手创建
    Manual,
}

/// 会话种类
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConversationKind {
    /// 联系人一对一投影
    Contact,
    /// 群组会话
    Group,
}

/// 会话聚合任务状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AggregateJobStatus {
    /// 首次待处理
    Pending,
    /// 持有租约处理中
    Processing,
    /// 已完成
    Completed,
    /// 失败后待重试
    Retry,
    /// 有限重试耗尽或永久错误，等待人工处理或新数据
    Failed,
}

/// 会话内消息分页方向
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PageDirection {
    /// 更旧（时间倒序「加载更多」）
    Older,
    /// 更新
    Newer,
}

/// 聚合结果终态 / 中间态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AggregationOutcome {
    Completed,
    Partial,
    Deferred,
    Skipped,
}
