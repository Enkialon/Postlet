//! 群组相关实体

use crate::domain::value_objects::{
    AccountId, EmailAddress, GroupId, GroupSource, ThreadId, TimestampMs,
};

/// 业务群组（自动 / 手动）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Group {
    pub id: GroupId,
    pub account_id: AccountId,
    pub name: String,
    /// 名称是否用户手动指定
    pub name_is_manual: bool,
    pub source: GroupSource,
    /// 发信成员是否手动锁定
    pub send_members_is_manual: bool,
    /// 被吸收时指向存活根群组
    pub merged_into_id: Option<GroupId>,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}

/// 线程 → 群组主归属（账户内每个 thread 最多一行）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GroupThread {
    pub account_id: AccountId,
    pub group_id: GroupId,
    pub thread_id: ThreadId,
    pub linked_at: TimestampMs,
}

/// 群组成员投影（观测 + 发信）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GroupParticipant {
    pub account_id: AccountId,
    pub group_id: GroupId,
    pub normalized_address: EmailAddress,
    pub display_name: String,
    /// 是否由邮件往来观测得到
    pub is_observed: bool,
    /// 是否发信成员
    pub is_send_member: bool,
    pub first_seen_at: TimestampMs,
    pub last_seen_at: TimestampMs,
}
