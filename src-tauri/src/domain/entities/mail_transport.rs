//! 账号邮件协议配置（与服务商品牌解耦）

use crate::domain::value_objects::{
    AccountId, MailAuthKind, MailTlsMode, ReceiveProtocol, SendProtocol, TimestampMs,
};

/// 账号收发信协议与端点
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AccountMailTransport {
    pub account_id: AccountId,
    pub receive_protocol: ReceiveProtocol,
    pub send_protocol: SendProtocol,
    pub auth_kind: MailAuthKind,
    /// 指向独立写入的受保护凭据；Graph 使用自己的 token 存储。
    pub credential_ref: Option<String>,
    pub imap_host: Option<String>,
    pub imap_port: Option<u16>,
    pub imap_tls: Option<MailTlsMode>,
    pub smtp_host: Option<String>,
    pub smtp_port: Option<u16>,
    pub smtp_tls: Option<MailTlsMode>,
    /// 登录用户名（常为邮箱）；OAuth2 时亦可作 XOAUTH2 user
    pub username: Option<String>,
    /// 服务端是否支持 IDLE；未探测为 None
    pub imap_idle_supported: Option<bool>,
    pub updated_at: TimestampMs,
}

impl AccountMailTransport {
    /// Outlook / Graph 默认配置
    pub fn graph_default(account_id: AccountId, credential_ref: String, now: TimestampMs) -> Self {
        Self {
            account_id,
            receive_protocol: ReceiveProtocol::Graph,
            send_protocol: SendProtocol::Graph,
            auth_kind: MailAuthKind::OAuth2,
            credential_ref: Some(credential_ref),
            imap_host: None,
            imap_port: None,
            imap_tls: None,
            smtp_host: None,
            smtp_port: None,
            smtp_tls: None,
            username: None,
            imap_idle_supported: None,
            updated_at: now,
        }
    }
}
