//! IMAP 连接与认证（TLS / STARTTLS / 密码 / XOAUTH2）
//!
//! 使用阻塞 `imap` crate；异步边界由 `spawn_blocking` 包裹。

use crate::application::error::PortError;
use crate::domain::value_objects::MailTlsMode;
use imap::Session;
use native_tls::TlsConnector;
use std::net::TcpStream;
use std::time::Duration;

const IMAP_IO_TIMEOUT: Duration = Duration::from_secs(60);

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

fn connect_tcp(endpoint: &ImapEndpoint) -> Result<TcpStream, PortError> {
    use std::net::ToSocketAddrs;
    let addr = (endpoint.host.as_str(), endpoint.port)
        .to_socket_addrs()
        .map_err(|e| PortError::Network {
            message: format!("IMAP 解析主机失败: {e}"),
        })?
        .next()
        .ok_or_else(|| PortError::Network {
            message: format!("IMAP 主机无可用地址: {}", endpoint.host),
        })?;
    let tcp = TcpStream::connect_timeout(&addr, IMAP_IO_TIMEOUT).map_err(|e| {
        PortError::Network {
            message: format!("IMAP 连接超时或失败: {e}"),
        }
    })?;
    let _ = tcp.set_read_timeout(Some(IMAP_IO_TIMEOUT));
    let _ = tcp.set_write_timeout(Some(IMAP_IO_TIMEOUT));
    Ok(tcp)
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
        MailTlsMode::Implicit => {
            let tcp = connect_tcp(endpoint)?;
            let tls_stream = tls.connect(&endpoint.host, tcp).map_err(|e| PortError::Network {
                message: format!("IMAP TLS 握手失败: {e}"),
            })?;
            imap::Client::new(tls_stream)
        }
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
    Ok(discover_folder_tree(session)?
        .into_iter()
        .map(|f| f.name)
        .collect())
}

#[derive(Debug, Clone)]
pub struct ImapFolderNode {
    pub name: String,
    pub parent_name: Option<String>,
    pub delimiter: Option<String>,
}

/// LIST 全树；按 delimiter 推导父路径（多级）。
pub fn discover_folder_tree(session: &mut ImapSession) -> Result<Vec<ImapFolderNode>, PortError> {
    let list = session.list(Some(""), Some("*")).map_err(|e| PortError::Remote {
        message: format!("IMAP LIST 失败: {e}"),
        retryable: true,
    })?;
    let mut out = Vec::with_capacity(list.len());
    for n in list.iter() {
        let name = n.name().to_string();
        let delimiter = n.delimiter().map(|d| d.to_string());
        let parent_name = delimiter.as_ref().and_then(|d| {
            if d.is_empty() {
                return None;
            }
            name.rsplit_once(d.as_str())
                .map(|(parent, _)| parent.to_string())
                .filter(|p| !p.is_empty())
        });
        out.push(ImapFolderNode {
            name,
            parent_name,
            delimiter,
        });
    }
    Ok(out)
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

/// 设置或清除 UID 的 `\Seen` 标志。
pub fn uid_set_seen(
    session: &mut ImapSession,
    folder: &str,
    uid: u32,
    is_read: bool,
) -> Result<(), PortError> {
    session
        .select(folder)
        .map_err(|e| PortError::Remote {
            message: format!("IMAP SELECT 失败: {e}"),
            retryable: true,
        })?;
    let seq = format!("{uid}");
    let result = if is_read {
        session.uid_store(&seq, "+FLAGS (\\Seen)")
    } else {
        session.uid_store(&seq, "-FLAGS (\\Seen)")
    };
    result.map_err(|e| PortError::Remote {
        message: format!("IMAP STORE FLAGS 失败: {e}"),
        retryable: true,
    })?;
    Ok(())
}

pub async fn uid_set_seen_async(
    endpoint: &ImapEndpoint,
    auth: &ImapAuth,
    folder: &str,
    uid: u32,
    is_read: bool,
) -> Result<(), PortError> {
    let endpoint = endpoint.clone();
    let auth = auth.clone();
    let folder = folder.to_string();
    tokio::task::spawn_blocking(move || {
        let mut session = session_login_blocking(&endpoint, &auth)?;
        let out = uid_set_seen(&mut session, &folder, uid, is_read);
        let _ = session.logout();
        out
    })
    .await
    .map_err(|e| PortError::Network {
        message: format!("IMAP STORE 任务失败: {e}"),
    })?
}
