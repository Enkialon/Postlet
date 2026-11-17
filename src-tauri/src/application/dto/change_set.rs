//! 事务提交后的本地变更摘要：数据范围 + 业务语义（origin）

use crate::domain::value_objects::{AccountId, MessageId, SessionId};

/// 一次成功提交产生的本地变更（在 commit 之后分发）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChangeSet {
    pub account_id: AccountId,
    pub session_ids: Vec<SessionId>,
    pub message_ids: Vec<MessageId>,
    /// 本轮真正新增的入站邮件；重复 upsert / outbound / 已读变更不得填入
    pub new_incoming_ids: Vec<MessageId>,
    /// 账号本地数据被整表清空（如清理数据）；前端应整列表失效
    #[serde(default)]
    pub account_cleared: bool,
    pub origin: ChangeOrigin,
}

/// 变更来源：决定是否允许系统通知等业务策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeOrigin {
    /// 历史灌库 / 首轮全量
    InitialSync,
    /// 增量同步来信
    IncrementalSync,
    /// 用户操作（已读、发送、配置等）
    UserAction,
    /// 聚合 / 配置触发的重投影
    Rebuild,
}

impl ChangeSet {
    pub fn empty(account_id: AccountId, origin: ChangeOrigin) -> Self {
        Self {
            account_id,
            session_ids: Vec::new(),
            message_ids: Vec::new(),
            new_incoming_ids: Vec::new(),
            account_cleared: false,
            origin,
        }
    }

    pub fn is_empty(&self) -> bool {
        !self.account_cleared
            && self.session_ids.is_empty()
            && self.message_ids.is_empty()
            && self.new_incoming_ids.is_empty()
    }
}

/// 推给前端的失效事件（不含邮件正文）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MailChangedEvent {
    pub account_id: String,
    /// 进程内单调；前端丢弃过期 generation
    pub generation: u64,
    pub origin: ChangeOrigin,
    pub session_ids: Vec<String>,
    pub message_ids: Vec<String>,
    pub new_incoming_count: usize,
    #[serde(default)]
    pub account_cleared: bool,
}

impl MailChangedEvent {
    pub fn from_change_set(changes: &ChangeSet, generation: u64) -> Self {
        Self {
            account_id: changes.account_id.as_str().to_owned(),
            generation,
            origin: changes.origin,
            session_ids: changes
                .session_ids
                .iter()
                .map(|id| id.as_str().to_owned())
                .collect(),
            message_ids: changes
                .message_ids
                .iter()
                .map(|id| id.as_str().to_owned())
                .collect(),
            new_incoming_count: changes.new_incoming_ids.len(),
            account_cleared: changes.account_cleared,
        }
    }
}
