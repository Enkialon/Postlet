//! 经校验的收发配置；协议变体关联各自的认证，不能组合出 Graph 密码或 IMAP OAuth。
use crate::domain::DomainError;
use crate::domain::value_objects::{AccountId, CredentialRef, MailTlsMode, ReceiveProtocol, SendProtocol, TimestampMs};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MailEndpoint { host: String, port: u16, tls: MailTlsMode }
impl MailEndpoint {
    pub fn new(host: String, port: u16, tls: MailTlsMode) -> Result<Self, DomainError> {
        if host.is_empty() || host.trim() != host || url::Host::parse(&host).is_err()
            || host.contains(['/', '@', '?', '#']) || port == 0 || tls == MailTlsMode::Plain {
            return Err(DomainError::InvariantViolation { message: "邮件端点须包含有效主机、端口以及 TLS/STARTTLS".into() });
        }
        Ok(Self { host, port, tls })
    }
    pub fn host(&self) -> &str { &self.host }
    pub fn port(&self) -> u16 { self.port }
    pub fn tls(&self) -> MailTlsMode { self.tls }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PasswordAuthConfig { username: String, credential_ref: CredentialRef }
impl PasswordAuthConfig {
    pub fn new(username: String, credential_ref: CredentialRef) -> Result<Self, DomainError> {
        Self::validate_username(&username)?;
        Ok(Self { username, credential_ref })
    }
    pub fn validate_username(username: &str) -> Result<(), DomainError> {
        if username.trim().is_empty() || username.trim() != username || username.chars().any(char::is_control) {
            return Err(DomainError::InvariantViolation { message: "登录用户名无效".into() });
        }
        Ok(())
    }
    pub fn username(&self) -> &str { &self.username }
    pub fn credential_ref(&self) -> &CredentialRef { &self.credential_ref }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct GraphAuthConfig { pub credential_ref: CredentialRef }

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PasswordMailConfig { pub endpoint: MailEndpoint, pub auth: PasswordAuthConfig }

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum ReceiveConfig { Graph(GraphAuthConfig), Imap(PasswordMailConfig) }
impl ReceiveConfig {
    pub fn protocol(&self) -> ReceiveProtocol { match self { Self::Graph(_) => ReceiveProtocol::Graph, Self::Imap(_) => ReceiveProtocol::Imap } }
    pub fn credential_ref(&self) -> &CredentialRef { match self { Self::Graph(auth) => &auth.credential_ref, Self::Imap(config) => config.auth.credential_ref() } }
    pub fn same_mailbox(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Graph(_), Self::Graph(_)) => true,
            (Self::Imap(a), Self::Imap(b)) => a.endpoint == b.endpoint && a.auth.username() == b.auth.username(),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum SendConfig { Graph(GraphAuthConfig), Smtp(PasswordMailConfig) }
impl SendConfig {
    pub fn protocol(&self) -> SendProtocol { match self { Self::Graph(_) => SendProtocol::Graph, Self::Smtp(_) => SendProtocol::Smtp } }
    pub fn credential_ref(&self) -> &CredentialRef { match self { Self::Graph(auth) => &auth.credential_ref, Self::Smtp(config) => config.auth.credential_ref() } }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AccountMailTransport {
    pub account_id: AccountId,
    pub receive: ReceiveConfig,
    pub send: SendConfig,
    pub imap_idle_supported: Option<bool>,
    pub updated_at: TimestampMs,
}
impl AccountMailTransport {
    pub fn graph(account_id: AccountId, credential_ref: CredentialRef, now: TimestampMs) -> Self {
        let auth = GraphAuthConfig { credential_ref };
        Self { account_id, receive: ReceiveConfig::Graph(auth.clone()), send: SendConfig::Graph(auth), imap_idle_supported: None, updated_at: now }
    }
}
