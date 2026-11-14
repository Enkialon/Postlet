//! 写入命令与列表查询命令

use crate::domain::value_objects::{
    AccountId, EmailAddress, GroupId, MailAuthKind, MailTlsMode, MessageId, ThreadId,
};

/// 登录后落库 / 刷新账户资料
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UpsertAccountCommand {
    pub id: AccountId,
    pub email: EmailAddress,
    pub display_name: String,
    pub aliases: Vec<EmailAddress>,
    pub tenant_id: Option<String>,
    pub home_account_id: Option<String>,
}

/// 更新账户显示配置
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UpdateAccountDisplaySettingsCommand {
    pub account_id: AccountId,
    pub show_group_messages_in_contact_conversation: bool,
    /// 是否纳入「全部账号」跨账号列表
    pub include_in_all: bool,
}

/// 新增 / 更新 IMAP+SMTP 账号（协议字段，非服务商品牌）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UpsertImapSmtpAccountCommand {
    pub email: String,
    pub display_name: String,
    pub auth_kind: MailAuthKind,
    pub imap_host: String,
    pub imap_port: u16,
    pub imap_tls: MailTlsMode,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_tls: MailTlsMode,
    pub username: Option<String>,
    pub password: Option<String>,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_at_ms: Option<i64>,
}

/// 批量标记已读 / 未读
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MarkMessagesReadCommand {
    pub account_id: AccountId,
    pub message_ids: Vec<MessageId>,
    pub is_read: bool,
}

/// 用户空手创建群组
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CreateManualGroupCommand {
    pub account_id: AccountId,
    pub name: String,
    pub send_member_addresses: Vec<EmailAddress>,
}

/// 重命名群组（同时标记 name_is_manual）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RenameGroupCommand {
    pub account_id: AccountId,
    pub group_id: GroupId,
    pub name: String,
}

/// 合并群组：source 吸收进 target（存活根）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MergeGroupsCommand {
    pub account_id: AccountId,
    pub source_group_id: GroupId,
    pub target_group_id: GroupId,
}

/// 把线程挪到另一群组（同事务迁移附着与两侧投影）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MoveThreadToGroupCommand {
    pub account_id: AccountId,
    pub thread_id: ThreadId,
    pub target_group_id: GroupId,
}

/// 更新发信成员集合
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UpdateGroupSendMembersCommand {
    pub account_id: AccountId,
    pub group_id: GroupId,
    pub send_member_addresses: Vec<EmailAddress>,
    /// true：锁定为手动，观测不再覆盖发信集合
    pub is_manual: bool,
}

/// 确保存在联系人会话（可顺带占位联系人）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EnsureContactConversationCommand {
    pub account_id: AccountId,
    pub normalized_address: EmailAddress,
    pub display_name: Option<String>,
}

/// 确保存在群组会话（解析到存活根群组）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EnsureGroupConversationCommand {
    pub account_id: AccountId,
    pub group_id: GroupId,
}

/// 联系人列表查询；`account_id` 为空时跨账号
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ContactListQuery {
    pub account_id: Option<AccountId>,
    pub cursor: Option<String>,
    pub limit: u32,
    /// 显示名 / 地址前缀
    pub search: Option<String>,
}

/// 群组列表查询；`account_id` 为空时跨账号
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GroupListQuery {
    pub account_id: Option<AccountId>,
    pub cursor: Option<String>,
    pub limit: u32,
    /// 默认 false：只返回根群组（merged_into_id 为空）
    pub include_merged: bool,
}
