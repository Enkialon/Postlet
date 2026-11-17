//! 写入命令与列表查询命令

use crate::domain::value_objects::{
    AccountId, EmailAddress, GroupId, MailTlsMode, MessageId, SessionId,
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
    /// 是否纳入「全部账号」跨账号列表
    pub include_in_all: bool,
}

/// 手动维护账户别名（self 额外身份；不做 Send-As）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UpdateAccountAliasesCommand {
    pub account_id: AccountId,
    /// 原始地址列表；用例侧 trim + lowercase 去重
    pub aliases: Vec<String>,
}

#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ImapAuthenticationInput { Password { username: String, password: String } }

#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SmtpAuthenticationInput { Shared, Password { username: String, password: String } }

/// 通用账号只接受密码；认证字段不包含 OAuth token 半支持分支。
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpsertImapSmtpAccountCommand {
    pub email: String,
    pub display_name: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub imap_tls: MailTlsMode,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_tls: MailTlsMode,
    pub imap_auth: ImapAuthenticationInput,
    pub smtp_auth: SmtpAuthenticationInput,
}

/// 批量标记已读 / 未读
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MarkMessagesReadCommand {
    pub account_id: AccountId,
    pub message_ids: Vec<MessageId>,
    pub is_read: bool,
}

/// 批量删除邮件（本地软删 + 推远端）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DeleteMessagesCommand {
    pub account_id: AccountId,
    pub message_ids: Vec<MessageId>,
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

/// 隐藏群组（隐藏群聊 Session；不删邮件）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HideGroupCommand {
    pub account_id: AccountId,
    pub group_id: GroupId,
}

/// 合并群聊 Session：source → target（消息迁入 + soft-link）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MergeSessionsCommand {
    pub account_id: AccountId,
    pub source_session_id: SessionId,
    pub target_session_id: SessionId,
}

/// 将选中消息改挂到目标 Session（私聊 / 群聊均可）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ReassignMessagesCommand {
    pub account_id: AccountId,
    pub message_ids: Vec<MessageId>,
    pub target_session_id: SessionId,
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

/// 确保存在私聊 Session（可顺带占位联系人）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EnsurePrivateSessionCommand {
    pub account_id: AccountId,
    pub normalized_address: EmailAddress,
    pub display_name: Option<String>,
}

/// 确保存在群聊 Session
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EnsureGroupSessionCommand {
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
    /// 群名 / 成员昵称或地址前缀
    pub search: Option<String>,
}
