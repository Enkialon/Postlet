//! 写入命令与列表查询命令

use crate::domain::value_objects::{
    AccountId, EmailAddress, GroupId, MessageId, ThreadId,
};

/// 登录后落库 / 刷新账户资料
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpsertAccountCommand {
    pub id: AccountId,
    pub email: EmailAddress,
    pub display_name: String,
    pub aliases: Vec<EmailAddress>,
    pub tenant_id: Option<String>,
    pub home_account_id: Option<String>,
}

/// 更新账户显示配置
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateAccountDisplaySettingsCommand {
    pub account_id: AccountId,
    pub show_group_messages_in_contact_conversation: bool,
}

/// 批量标记已读 / 未读
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkMessagesReadCommand {
    pub account_id: AccountId,
    pub message_ids: Vec<MessageId>,
    pub is_read: bool,
}

/// 用户空手创建群组
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateManualGroupCommand {
    pub account_id: AccountId,
    pub name: String,
    pub send_member_addresses: Vec<EmailAddress>,
}

/// 重命名群组（同时标记 name_is_manual）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameGroupCommand {
    pub account_id: AccountId,
    pub group_id: GroupId,
    pub name: String,
}

/// 合并群组：source 吸收进 target（存活根）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeGroupsCommand {
    pub account_id: AccountId,
    pub source_group_id: GroupId,
    pub target_group_id: GroupId,
}

/// 把线程挪到另一群组（同事务迁移附着与两侧投影）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveThreadToGroupCommand {
    pub account_id: AccountId,
    pub thread_id: ThreadId,
    pub target_group_id: GroupId,
}

/// 更新发信成员集合
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateGroupSendMembersCommand {
    pub account_id: AccountId,
    pub group_id: GroupId,
    pub send_member_addresses: Vec<EmailAddress>,
    /// true：锁定为手动，观测不再覆盖发信集合
    pub is_manual: bool,
}

/// 确保存在联系人会话（可顺带占位联系人）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnsureContactConversationCommand {
    pub account_id: AccountId,
    pub normalized_address: EmailAddress,
    pub display_name: Option<String>,
}

/// 联系人列表查询
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactListQuery {
    pub account_id: AccountId,
    pub cursor: Option<String>,
    pub limit: u32,
    /// 显示名 / 地址前缀
    pub search: Option<String>,
}

/// 群组列表查询
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupListQuery {
    pub account_id: AccountId,
    pub cursor: Option<String>,
    pub limit: u32,
    /// 默认 false：只返回根群组（merged_into_id 为空）
    pub include_merged: bool,
}
