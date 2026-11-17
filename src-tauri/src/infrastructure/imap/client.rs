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
        ImapAuth::Password { username, password } => {
            let username = username.trim();
            let password = password.trim();
            client.login(username, password).map_err(|(e, _)| PortError::Auth {
                message: format!("IMAP 密码登录失败: {e}"),
            })
        }
        ImapAuth::OAuth2 {
            username,
            access_token,
        } => {
            let xoauth = Xoauth2 {
                user: username.trim().to_string(),
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

/// 仅清除指定 UID；没有 UIDPLUS 时不产生任何删除副作用。
pub fn uid_delete<T: std::io::Read + std::io::Write>(
    session: &mut Session<T>,
    folder: &str,
    uid: u32,
) -> Result<(), PortError> {
    let capabilities = session.capabilities().map_err(|_| PortError::Remote {
        message: "IMAP 删除能力检查失败，尚未标记删除".into(),
        retryable: true,
    })?;
    if !capabilities.has_str("UIDPLUS") {
        return Err(PortError::Remote {
            message: "服务器不支持 UIDPLUS，无法安全永久删除单封邮件".into(),
            retryable: false,
        });
    }
    session
        .select(folder)
        .map_err(|e| PortError::Remote {
            message: format!("IMAP SELECT 失败: {e}"),
            retryable: true,
        })?;
    let seq = format!("{uid}");
    session
        .uid_store(&seq, "+FLAGS (\\Deleted)")
        .map_err(|e| PortError::Remote {
            message: format!("IMAP STORE \\Deleted 未确认，目标可能已标记删除: {e}"),
            retryable: true,
        })?;
    session.uid_expunge(&seq).map_err(|e| PortError::Remote {
        message: format!("目标已标记 \\Deleted；UID EXPUNGE 未确认，未执行整夹清理: {e}"),
        retryable: true,
    })?;
    Ok(())
}

pub async fn uid_delete_async(
    endpoint: &ImapEndpoint,
    auth: &ImapAuth,
    folder: &str,
    uid: u32,
) -> Result<(), PortError> {
    let endpoint = endpoint.clone();
    let auth = auth.clone();
    let folder = folder.to_string();
    tokio::task::spawn_blocking(move || {
        let mut session = session_login_blocking(&endpoint, &auth)?;
        let out = uid_delete(&mut session, &folder, uid);
        let _ = session.logout();
        out
    })
    .await
    .map_err(|e| PortError::Network {
        message: format!("IMAP 删除任务失败: {e}"),
    })?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read, Write};
    use std::sync::{Arc, Mutex};

    struct ScriptedStream {
        responses: Cursor<Vec<u8>>,
        commands: Arc<Mutex<Vec<u8>>>,
    }

    impl Read for ScriptedStream {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.responses.read(buf)
        }
    }

    impl Write for ScriptedStream {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.commands.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn delete_with_responses(responses: &str) -> (Result<(), PortError>, String) {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let stream = ScriptedStream {
            responses: Cursor::new(format!("a1 OK login\r\n{responses}").into_bytes()),
            commands: commands.clone(),
        };
        let mut session = imap::Client::new(stream).login("user", "secret")
            .map_err(|(err, _)| err).unwrap();
        let result = uid_delete(&mut session, "INBOX", 20);
        let written = String::from_utf8(commands.lock().unwrap().clone()).unwrap();
        (result, written)
    }

    #[test]
    fn deletion_without_uidplus_has_no_side_effects() {
        let (result, commands) = delete_with_responses(
            "* CAPABILITY IMAP4rev1\r\na2 OK capabilities\r\n",
        );
        assert!(matches!(result, Err(PortError::Remote { retryable: false, .. })));
        assert!(!commands.contains("SELECT"));
        assert!(!commands.contains("STORE"));
        assert!(!commands.contains("EXPUNGE"));
    }

    #[test]
    fn deletion_never_falls_back_to_mailbox_expunge() {
        // UID 10 was already Deleted by another client. Only UID 20 is ours.
        for completion in ["a5 OK deleted\r\n", "a5 NO refused\r\n", "a5 BAD unsupported\r\n", ""] {
            let responses = format!(
                "* CAPABILITY IMAP4rev1 UIDPLUS\r\na2 OK capabilities\r\n\
                 * FLAGS (\\Seen \\Deleted)\r\n* 2 EXISTS\r\n\
                 * OK [UIDVALIDITY 1] generation\r\na3 OK selected\r\n\
                 * 2 FETCH (UID 20 FLAGS (\\Deleted))\r\na4 OK stored\r\n{completion}"
            );
            let (result, commands) = delete_with_responses(&responses);
            assert_eq!(result.is_ok(), completion.starts_with("a5 OK"));
            assert!(commands.contains("UID STORE 20 +FLAGS (\\Deleted)"));
            assert!(commands.contains("UID EXPUNGE 20"));
            assert!(!commands.lines().any(|line| line.ends_with(" EXPUNGE")));
            if let Err(error) = result {
                assert!(error.to_string().contains("已标记"));
            }
        }
    }
}
