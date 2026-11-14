//! IMAP 连接与认证（TLS / STARTTLS / 密码 / XOAUTH2）
//!
//! 使用阻塞 `imap` crate；异步边界由 `spawn_blocking` 包裹。

use crate::application::error::PortError;
use crate::domain::value_objects::MailTlsMode;
use imap::Session;
use native_tls::TlsConnector;
use std::net::TcpStream;

#[derive(Debug, Clone)]
pub struct ImapEndpoint {
    pub host: String,
    pub port: u16,
    pub tls: MailTlsMode,
}

#[derive(Debug, Clone)]
pub enum ImapAuth {
    Password { username: String, password: String },
    OAuth2 { username: String, access_token: String },
}

pub type ImapSession = Session<native_tls::TlsStream<TcpStream>>;

struct Xoauth2 {
    user: String,
    access_token: String,
}

impl imap::Authenticator for Xoauth2 {
    type Response = String;
    fn process(&self, _challenge: &[u8]) -> Self::Response {
        let raw = format!(
            "user={}\x01auth=Bearer {}\x01\x01",
            self.user, self.access_token
        );
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, raw)
    }
}

/// 阻塞登录。
pub fn session_login_blocking(
    endpoint: &ImapEndpoint,
    auth: &ImapAuth,
) -> Result<ImapSession, PortError> {
    let tls = TlsConnector::builder()
        .build()
        .map_err(|e| PortError::Network {
            message: format!("TLS 构建失败: {e}"),
        })?;

    let client = match endpoint.tls {
        MailTlsMode::Implicit => imap::connect(
            (endpoint.host.as_str(), endpoint.port),
            &endpoint.host,
            &tls,
        )
        .map_err(|e| PortError::Network {
            message: format!("IMAP TLS 连接失败: {e}"),
        })?,
        MailTlsMode::StartTls => imap::connect_starttls(
            (endpoint.host.as_str(), endpoint.port),
            &endpoint.host,
            &tls,
        )
        .map_err(|e| PortError::Network {
            message: format!("IMAP STARTTLS 连接失败: {e}"),
        })?,
        MailTlsMode::Plain => {
            return Err(PortError::Remote {
                message: "明文 IMAP 已禁用，请使用 TLS 或 STARTTLS".into(),
                retryable: false,
            });
        }
    };

    match auth {
        ImapAuth::Password { username, password } => client
            .login(username, password)
            .map_err(|(e, _)| PortError::Auth {
                message: format!("IMAP 密码登录失败: {e}"),
            }),
        ImapAuth::OAuth2 {
            username,
            access_token,
        } => {
            let xoauth = Xoauth2 {
                user: username.clone(),
                access_token: access_token.clone(),
            };
            client
                .authenticate("XOAUTH2", &xoauth)
                .map_err(|(e, _)| PortError::Auth {
                    message: format!("IMAP XOAUTH2 失败: {e}"),
                })
        }
    }
}

pub async fn session_login(
    endpoint: &ImapEndpoint,
    auth: &ImapAuth,
) -> Result<ImapSession, PortError> {
    let endpoint = endpoint.clone();
    let auth = auth.clone();
    tokio::task::spawn_blocking(move || session_login_blocking(&endpoint, &auth))
        .await
        .map_err(|e| PortError::Network {
            message: format!("IMAP 登录任务失败: {e}"),
        })?
}

pub fn discover_folders(session: &mut ImapSession) -> Result<Vec<String>, PortError> {
    let list = session.list(Some(""), Some("*")).map_err(|e| PortError::Remote {
        message: format!("IMAP LIST 失败: {e}"),
        retryable: true,
    })?;
    Ok(list.iter().map(|n| n.name().to_string()).collect())
}

pub fn list_capabilities(session: &mut ImapSession) -> Result<(bool, Vec<String>), PortError> {
    let caps = session.capabilities().map_err(|e| PortError::Remote {
        message: format!("IMAP CAPABILITY 失败: {e}"),
        retryable: true,
    })?;
    let names: Vec<String> = caps.iter().map(|c| format!("{c:?}")).collect();
    let idle = caps.has_str("IDLE");
    Ok((idle, names))
}
