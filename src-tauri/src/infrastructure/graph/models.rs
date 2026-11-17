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

/// 远端账户身份（不含本地同步与显示配置）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteAccount {
    /// 与本地 AccountId 绑定后的标识（由 Auth 基础设施映射）
    pub account_id: String,
    pub email: EmailAddress,
    pub display_name: String,
    pub aliases: Vec<EmailAddress>,
    pub tenant_id: Option<String>,
    pub home_account_id: Option<String>,
}
