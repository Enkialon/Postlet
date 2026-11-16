//! 群组投影实体（关联群聊 Session；非消息上级）

use crate::domain::value_objects::{AccountId, EmailAddress, GroupId, GroupSource, TimestampMs};

/// 群档案：名称与成员；聊天状态在 Session
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
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
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
