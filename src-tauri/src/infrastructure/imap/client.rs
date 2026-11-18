//! IMAP 连接与认证（TLS / STARTTLS / 密码 / XOAUTH2）
//!
//! 使用阻塞 `imap` crate；异步边界由 `spawn_blocking` 包裹。

use crate::infrastructure::error::InfraError;
use crate::domain::value_objects::{MailTlsMode, RemoteKey};
use super::parse::decode_remote_key;
use imap::Session;
use crate::infrastructure::mail::operation::MailOperation;
use std::time::Duration;


#[derive(Debug, Clone)]
pub struct ImapEndpoint {
    pub host: String,
    pub port: u16,
    pub tls: MailTlsMode,
}

#[derive(Clone)]
pub enum ImapAuth {
    Password { username: String, password: String },
    OAuth2 { username: String, access_token: String },
}

pub type ImapSession = Session<super::stream::ImapStream>;

pub async fn load_connection(
    credentials_dir: &std::path::Path,
    config: &crate::domain::entities::PasswordMailConfig,
) -> Result<(ImapEndpoint, ImapAuth), InfraError> {
    let secret = crate::infrastructure::mail::credentials::load(credentials_dir, &config.auth).await?
        .ok_or_else(|| InfraError::Auth { message: "缺少 IMAP 密码或邮箱授权码".into() })?;
    Ok((ImapEndpoint { host: config.endpoint.host().to_owned(), port: config.endpoint.port(), tls: config.endpoint.tls() },
        ImapAuth::Password { username: config.auth.username().to_owned(), password: secret.password }))
}

pub fn discover_folders(session: &mut ImapSession) -> Result<Vec<String>, InfraError> {
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
    /// LIST 返回的原始属性（含 `\Junk` 等 SPECIAL-USE；服务器不支持时为空）
    pub attributes: Vec<String>,
}

/// LIST 全树；按 delimiter 推导父路径（多级）。
pub fn discover_folder_tree(session: &mut ImapSession) -> Result<Vec<ImapFolderNode>, InfraError> {
    let list = session.list(Some(""), Some("*")).map_err(|e| InfraError::Remote {
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
        let attributes = n
            .attributes()
            .iter()
            .map(|a| format!("{a:?}").trim_matches('"').to_string())
            .collect();
        out.push(ImapFolderNode {
            name,
            parent_name,
            delimiter,
            attributes,
        });
    }
    Ok(out)
}

pub fn list_capabilities(session: &mut ImapSession) -> Result<(bool, Vec<String>), InfraError> {
    let caps = session.capabilities().map_err(|e| InfraError::Remote {
        message: format!("IMAP CAPABILITY 失败: {e}"),
        retryable: true,
    })?;
    let names: Vec<String> = caps.iter().map(|c| format!("{c:?}")).collect();
    let idle = caps.has_str("IDLE");
    Ok((idle, names))
}

/// 设置或清除 UID 的 `\Seen` 标志。
pub fn uid_set_seen<T: std::io::Read + std::io::Write>(
    session: &mut Session<T>,
    folder: &str,
    remote_key: &RemoteKey,
    is_read: bool,
) -> Result<(), InfraError> {
    let (generation, uid) = decode_remote_key(remote_key)?;
    let selected = session.select(folder).map_err(|_| InfraError::Remote {
        message: "IMAP SELECT 失败".into(), retryable: true,
    })?;
    if selected.uid_validity != Some(generation) {
        return Err(InfraError::Conflict { message: "IMAP UIDVALIDITY 已改变，旧操作已停止，请同步后重新操作".into() });
    }
    let seq = format!("{uid}");
    let result = if is_read {
        session.uid_store(&seq, "+FLAGS (\\Seen)")
    } else {
        session.uid_store(&seq, "-FLAGS (\\Seen)")
    };
    result.map_err(|e| InfraError::Remote {
        message: format!("IMAP STORE FLAGS 失败: {e}"),
        retryable: true,
    })?;
    Ok(())
}

pub async fn uid_set_seen_async(
    endpoint: &ImapEndpoint,
    auth: &ImapAuth,
    folder: &str,
    remote_key: &RemoteKey,
    is_read: bool,
) -> Result<(), InfraError> {
    let operation = MailOperation::new(Duration::from_secs(180));
    let mut session = super::connection::session_login(endpoint, auth, &operation).await?;
    let folder = folder.to_string();
    let remote_key = remote_key.clone();
    operation.run_blocking(move || {
        let out = uid_set_seen(&mut session, &folder, &remote_key, is_read);
        let _ = session.logout();
        out
    })
    .await
}

/// 仅清除指定 UID；没有 UIDPLUS 时不产生任何删除副作用。
/// 移动到目标文件夹：优先 MOVE；不支持时 COPY + 单封 UID EXPUNGE。
///
/// 任何阶段未知都返回可重试错误，绝不回退整夹 EXPUNGE。
pub fn uid_move<T: std::io::Read + std::io::Write>(
    session: &mut Session<T>,
    folder: &str,
    remote_key: &RemoteKey,
    destination: &str,
) -> Result<(), InfraError> {
    let capabilities = session.capabilities().map_err(|_| InfraError::Remote {
        message: "IMAP 能力检查失败，尚未移动".into(),
        retryable: true,
    })?;
    let (generation, uid) = decode_remote_key(remote_key)?;
    let selected = session.select(folder).map_err(|_| InfraError::Remote {
        message: "IMAP SELECT 失败".into(),
        retryable: true,
    })?;
    if selected.uid_validity != Some(generation) {
        return Err(InfraError::Conflict {
            message: "IMAP UIDVALIDITY 已改变，旧移动已停止，请同步后重新操作".into(),
        });
    }
    let seq = format!("{uid}");

    if capabilities.has_str("MOVE") {
        return session
            .uid_mv(&seq, destination)
            .map(|_| ())
            .map_err(|e| InfraError::Remote {
                message: format!("IMAP MOVE 未确认，结果未知: {e}"),
                retryable: false,
            });
    }

    if !capabilities.has_str("UIDPLUS") {
        return Err(InfraError::Remote {
            message: "服务器不支持 MOVE 或 UIDPLUS，无法安全移动单封邮件".into(),
            retryable: false,
        });
    }
    session
        .uid_copy(&seq, destination)
        .map_err(|e| InfraError::Remote {
            message: format!("IMAP COPY 未确认，源邮件保留: {e}"),
            retryable: true,
        })?;
    session
        .uid_store(&seq, r"+FLAGS (\Deleted)")
        .map_err(|e| InfraError::Remote {
            message: format!("COPY 已完成但源邮件标记未确认，目标可能存在副本: {e}"),
            retryable: false,
        })?;
    session
        .uid_expunge(&seq)
        .map_err(|e| InfraError::Remote {
            message: format!("COPY 已完成但源邮件未清除，目标可能存在副本: {e}"),
            retryable: false,
        })?;
    Ok(())
}

pub fn uid_delete<T: std::io::Read + std::io::Write>(
    session: &mut Session<T>,
    folder: &str,
    remote_key: &RemoteKey,
) -> Result<(), InfraError> {
    let capabilities = session.capabilities().map_err(|_| InfraError::Remote {
        message: "IMAP 删除能力检查失败，尚未标记删除".into(),
        retryable: true,
    })?;
    if !capabilities.has_str("UIDPLUS") {
        return Err(InfraError::Remote {
            message: "服务器不支持 UIDPLUS，无法安全永久删除单封邮件".into(),
            retryable: false,
        });
    }
    let (generation, uid) = decode_remote_key(remote_key)?;
    let selected = session.select(folder).map_err(|_| InfraError::Remote {
        message: "IMAP SELECT 失败".into(), retryable: true,
    })?;
    if selected.uid_validity != Some(generation) {
        return Err(InfraError::Conflict { message: "IMAP UIDVALIDITY 已改变，旧操作已停止，请同步后重新操作".into() });
    }
    let seq = format!("{uid}");
    session
        .uid_store(&seq, "+FLAGS (\\Deleted)")
        .map_err(|e| InfraError::Remote {
            message: format!("IMAP STORE \\Deleted 未确认，目标可能已标记删除: {e}"),
            retryable: true,
        })?;
    session.uid_expunge(&seq).map_err(|e| InfraError::Remote {
        message: format!("目标已标记 \\Deleted；UID EXPUNGE 未确认，未执行整夹清理: {e}"),
        retryable: true,
    })?;
    Ok(())
}

pub async fn uid_delete_async(
    endpoint: &ImapEndpoint,
    auth: &ImapAuth,
    folder: &str,
    remote_key: &RemoteKey,
) -> Result<(), InfraError> {
    let operation = MailOperation::new(Duration::from_secs(180));
    let mut session = super::connection::session_login(endpoint, auth, &operation).await?;
    let folder = folder.to_string();
    let remote_key = remote_key.clone();
    operation.run_blocking(move || {
        let out = uid_delete(&mut session, &folder, &remote_key);
        let _ = session.logout();
        out
    })
    .await
}

/// APPEND 结果三态：成功 = Some(true)，服务器明确拒绝 = Some(false)，
/// 连接/响应未知 = None（调用方记 uncertain，不重复 APPEND）。
pub fn append_message<T: std::io::Read + std::io::Write>(
    session: &mut Session<T>,
    folder: &str,
    mime: &[u8],
) -> Result<Option<bool>, InfraError> {
    classify_append_result(session.append(folder, mime))
}

fn classify_append_result(
    result: Result<(), imap::Error>,
) -> Result<Option<bool>, InfraError> {
    match result {
        Ok(()) => Ok(Some(true)),
        Err(imap::Error::No(message)) | Err(imap::Error::Bad(message)) => {
            eprintln!("IMAP APPEND 被服务器拒绝: {message}");
            Ok(Some(false))
        }
        Err(imap::Error::Append) => Ok(Some(false)),
        Err(imap::Error::ConnectionLost) | Err(imap::Error::Io(_)) | Err(imap::Error::Parse(_)) => {
            Ok(None)
        }
        Err(error) => Err(InfraError::Remote {
            message: format!("IMAP APPEND 未确认: {error}"),
            retryable: true,
        }),
    }
}

pub async fn append_message_async(
    endpoint: &ImapEndpoint,
    auth: &ImapAuth,
    folder: &str,
    mime: &[u8],
) -> Result<Option<bool>, InfraError> {
    let operation = MailOperation::new(Duration::from_secs(180));
    let mut session = super::connection::session_login(endpoint, auth, &operation).await?;
    let folder = folder.to_string();
    let mime = mime.to_vec();
    operation
        .run_blocking(move || {
            let out = append_message(&mut session, &folder, &mime);
            let _ = session.logout();
            out
        })
        .await
}

pub async fn uid_move_async(
    endpoint: &ImapEndpoint,
    auth: &ImapAuth,
    folder: &str,
    remote_key: &RemoteKey,
    destination: &str,
) -> Result<(), InfraError> {
    let operation = MailOperation::new(Duration::from_secs(180));
    let mut session = super::connection::session_login(endpoint, auth, &operation).await?;
    let folder = folder.to_string();
    let remote_key = remote_key.clone();
    let destination = destination.to_string();
    operation
        .run_blocking(move || {
            let out = uid_move(&mut session, &folder, &remote_key, &destination);
            let _ = session.logout();
            out
        })
        .await
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

    fn delete_with_responses(responses: &str) -> (Result<(), InfraError>, String) {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let stream = ScriptedStream {
            responses: Cursor::new(format!("a1 OK login\r\n{responses}").into_bytes()),
            commands: commands.clone(),
        };
        let mut session = imap::Client::new(stream).login("user", "secret")
            .map_err(|(err, _)| err).unwrap();
        let result = uid_delete(&mut session, "INBOX", &super::super::parse::remote_id(1, 20));
        let written = String::from_utf8(commands.lock().unwrap().clone()).unwrap();
        (result, written)
    }

    #[test]
    fn append_connection_loss_is_unknown_not_a_definite_rejection() {
        assert_eq!(classify_append_result(Ok(())).unwrap(), Some(true));
        assert_eq!(classify_append_result(Err(imap::Error::No("no".into()))).unwrap(), Some(false));
        assert_eq!(classify_append_result(Err(imap::Error::Append)).unwrap(), Some(false));
        assert_eq!(classify_append_result(Err(imap::Error::ConnectionLost)).unwrap(), None);
        assert_eq!(
            classify_append_result(Err(imap::Error::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "lost"
            ))))
            .unwrap(),
            None
        );
    }

    #[test]
    fn deletion_without_uidplus_has_no_side_effects() {
        let (result, commands) = delete_with_responses(
            "* CAPABILITY IMAP4rev1\r\na2 OK capabilities\r\n",
        );
        assert!(matches!(result, Err(InfraError::Remote { retryable: false, .. })));
        assert!(!commands.contains("SELECT"));
        assert!(!commands.contains("STORE"));
        assert!(!commands.contains("EXPUNGE"));
    }

    #[test]
    fn generation_mismatch_or_missing_validity_never_mutates_mail() {
        for selected in ["* OK [UIDVALIDITY 2] generation\r\n", ""] {
            let responses = format!("* CAPABILITY IMAP4rev1 UIDPLUS\r\na2 OK capabilities\r\n* FLAGS (\\Seen \\Deleted)\r\n* 1 EXISTS\r\n{selected}a3 OK selected\r\n");
            let (result, commands) = delete_with_responses(&responses);
            assert!(matches!(result, Err(InfraError::Conflict { .. })), "{result:?}");
            assert!(!commands.contains("STORE"));
            assert!(!commands.contains("EXPUNGE"));

            let commands = Arc::new(Mutex::new(Vec::new()));
            let stream = ScriptedStream {
                responses: Cursor::new(format!("a1 OK login\r\n* FLAGS (\\Seen \\Deleted)\r\n* 1 EXISTS\r\n{selected}a2 OK selected\r\n").into_bytes()),
                commands: commands.clone(),
            };
            let mut session = imap::Client::new(stream).login("user", "secret").map_err(|(error, _)| error).unwrap();
            let result = uid_set_seen(&mut session, "INBOX", &RemoteKey::new("imap:1:20"), true);
            assert!(matches!(result, Err(InfraError::Conflict { .. })), "{result:?}");
            assert!(!String::from_utf8(commands.lock().unwrap().clone()).unwrap().contains("STORE"));
        }
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
