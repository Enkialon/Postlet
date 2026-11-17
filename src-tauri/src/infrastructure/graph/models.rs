//! Graph 适配器的联系人和登录结果。
use crate::domain::value_objects::EmailAddress;

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

/// 远端账户身份（不含本地同步与显示配置；Graph provider 元数据只在 token 缓存）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteAccount {
    /// Graph `/me` 的 oid；不作为本地 Account 业务主键
    pub account_id: String,
    pub email: EmailAddress,
    pub display_name: String,
    pub aliases: Vec<EmailAddress>,
}
