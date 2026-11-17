//! 有界连接：DNS、地址回退、TLS、问候与认证共用一个截止时间。

use super::client::{ImapAuth, ImapEndpoint, ImapSession};
use super::stream::{DeadlineTcp, ImapStream};
use crate::domain::value_objects::MailTlsMode;
use crate::infrastructure::error::InfraError;
use crate::infrastructure::mail::operation::MailOperation;
use std::net::{SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const LOGIN_TIMEOUT: Duration = Duration::from_secs(60);
const ADDRESS_TIMEOUT: Duration = Duration::from_secs(3);

fn network(message: impl Into<String>) -> InfraError {
    InfraError::Network {
        message: message.into(),
    }
}

fn remaining(deadline: Instant) -> Result<Duration, InfraError> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| network("IMAP 连接或认证超时"))
}

fn connect_tcp(
    addresses: &[SocketAddr],
    deadline: Instant,
    operation: &MailOperation,
) -> Result<TcpStream, InfraError> {
    for address in addresses {
        operation.checkpoint()?;
        let timeout = remaining(deadline)?.min(ADDRESS_TIMEOUT);
        let tcp = match TcpStream::connect_timeout(address, timeout) {
            Ok(tcp) => tcp,
            Err(_) => continue,
        };
        return Ok(tcp);
    }
    Err(network("IMAP 所有解析地址均连接失败"))
}

fn authenticate<T: std::io::Read + std::io::Write>(
    client: imap::Client<T>,
    auth: &ImapAuth,
) -> Result<imap::Session<T>, InfraError> {
    match auth {
        ImapAuth::Password { username, password } => {
            // 密码是精确的秘密值；空格可能是合法组成部分。
            client
                .login(username.trim(), password)
                .map_err(|(error, _)| match error {
                    imap::error::Error::No(_) | imap::error::Error::Bad(_) => InfraError::Auth {
                        message: "IMAP 密码登录被服务器拒绝".into(),
                    },
                    _ => network("IMAP 登录响应未确认"),
                })
        }
        ImapAuth::OAuth2 { .. } => unreachable!("OAuth2 rejected before DNS"),
    }
}

/// imap 2.4 的 Client::secure 仅接受 TcpStream；受控流在认证前按协议升级。
/// 仍使用锁定的 imap-proto 解析响应，未获同 tag 的 OK 绝不进入 TLS/认证。
fn request_starttls<T: std::io::Read + std::io::Write>(stream: T) -> Result<T, InfraError> {
    use imap_proto::{Response, Status};
    use std::io::{BufRead, Read};
    let mut reader = std::io::BufReader::new(stream);
    let mut remaining_bytes = 64 * 1024usize;
    let mut read_line = |reader: &mut std::io::BufReader<T>| -> Result<Vec<u8>, InfraError> {
        let mut line = Vec::new();
        Read::take(reader, (remaining_bytes + 1) as u64)
            .read_until(b'\n', &mut line)
            .map_err(|_| network("IMAP STARTTLS 响应读取失败"))?;
        if line.len() > remaining_bytes || !line.ends_with(b"\r\n") {
            return Err(network("IMAP STARTTLS 响应过长或不完整"));
        }
        remaining_bytes -= line.len();
        Ok(line)
    };
    let greeting = read_line(&mut reader)?;
    if !matches!(
        imap_proto::parse_response(&greeting),
        Ok((
            [],
            Response::Data {
                status: Status::Ok,
                ..
            }
        ))
    ) {
        return Err(network("IMAP STARTTLS 问候无效"));
    }
    reader
        .get_mut()
        .write_all(b"a1 STARTTLS\r\n")
        .map_err(|_| network("IMAP STARTTLS 命令发送失败"))?;
    reader
        .get_mut()
        .flush()
        .map_err(|_| network("IMAP STARTTLS 命令发送失败"))?;
    loop {
        let line = read_line(&mut reader)?;
        match imap_proto::parse_response(&line) {
            Ok((
                [],
                Response::Done {
                    tag,
                    status: Status::Ok,
                    ..
                },
            )) if tag.as_bytes() == b"a1" => break,
            Ok(([], Response::Capabilities(_)))
            | Ok((
                [],
                Response::Data {
                    status: Status::Ok, ..
                },
            )) => continue,
            _ => return Err(network("IMAP STARTTLS 升级未获确认")),
        }
    }
    if !reader.buffer().is_empty() {
        return Err(network("IMAP STARTTLS 后出现非预期明文数据"));
    }
    Ok(reader.into_inner())
}

fn login_resolved(
    endpoint: ImapEndpoint,
    auth: ImapAuth,
    addresses: Vec<SocketAddr>,
    deadline: Instant,
    operation: MailOperation,
) -> Result<ImapSession, InfraError> {
    let tcp = connect_tcp(&addresses, deadline, &operation)?;
    let phase = Arc::new(Mutex::new(deadline));
    let tcp = DeadlineTcp::new(tcp, &operation, phase.clone())?;
    let tls = native_tls::TlsConnector::builder()
        .build()
        .map_err(|_| network("IMAP TLS 初始化失败"))?;
    let tcp = if endpoint.tls == MailTlsMode::StartTls {
        request_starttls(tcp)?
    } else {
        tcp
    };
    let stream = tls
        .connect(&endpoint.host, tcp)
        .map_err(|_| network("IMAP TLS 握手失败"))?;
    let mut client = imap::Client::new(ImapStream(stream));
    if endpoint.tls == MailTlsMode::Implicit {
        client
            .read_greeting()
            .map_err(|_| network("IMAP 问候失败"))?;
    }
    let session = authenticate(client, &auth)?;
    *phase.lock().unwrap_or_else(|e| e.into_inner()) = operation.deadline();
    Ok(session)
}

pub async fn session_login(
    endpoint: &ImapEndpoint,
    auth: &ImapAuth,
    operation: &MailOperation,
) -> Result<ImapSession, InfraError> {
    if matches!(auth, ImapAuth::OAuth2 { .. }) {
        return Err(InfraError::Auth {
            message: "通用 IMAP V1 不支持 OAuth2，请使用密码或邮箱授权码".into(),
        });
    }
    if endpoint.tls == MailTlsMode::Plain {
        return Err(InfraError::Remote {
            message: "明文 IMAP 已禁用，请使用 TLS 或 STARTTLS".into(),
            retryable: false,
        });
    }
    let guard = operation.cancel_on_drop();
    let deadline = operation.phase_deadline(LOGIN_TIMEOUT);
    let host = endpoint.host.clone();
    let port = endpoint.port;
    let addresses = operation
        .wait_until(deadline, async {
            tokio::net::lookup_host((host.as_str(), port))
                .await
                .map(|addresses| addresses.collect::<Vec<_>>())
                .map_err(|_| network("IMAP DNS 解析失败"))
        })
        .await?;
    let endpoint = endpoint.clone();
    let auth = auth.clone();
    let control = operation.clone();
    let result = operation
        .run_blocking_until(deadline, move || {
            login_resolved(endpoint, auth, addresses, deadline, control)
        })
        .await;
    guard.disarm();
    result
}

#[cfg(test)]
async fn session_login_with_timeout(
    endpoint: &ImapEndpoint,
    auth: &ImapAuth,
    budget: Duration,
) -> Result<ImapSession, InfraError> {
    session_login(endpoint, auth, &MailOperation::new(budget)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    #[test]
    fn starttls_requires_valid_greeting_and_matching_ok_without_extra_plaintext() {
        struct Stream {
            input: Cursor<Vec<u8>>,
            output: Arc<Mutex<Vec<u8>>>,
        }
        impl Read for Stream {
            fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
                self.input.read(bytes)
            }
        }
        impl Write for Stream {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.output.lock().unwrap().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        for (response, expected) in [
            ("* OK hello\r\na1 OK upgrade\r\n", true),
            (
                "* OK hello\r\n* CAPABILITY IMAP4rev1 STARTTLS\r\na1 OK upgrade\r\n",
                true,
            ),
            ("* BYE refused\r\n", false),
            ("garbage\r\n", false),
            ("* OK hello\r\na1 NO refused\r\n", false),
            ("* OK hello\r\na2 OK wrong tag\r\n", false),
            ("* OK hello\r\na1 OK upgrade\r\nextra plaintext\r\n", false),
        ] {
            let output = Arc::new(Mutex::new(Vec::new()));
            let stream = Stream {
                input: Cursor::new(response.as_bytes().to_vec()),
                output: output.clone(),
            };
            assert_eq!(request_starttls(stream).is_ok(), expected, "{response}");
            let command = output.lock().unwrap().clone();
            assert!(command.is_empty() || command == b"a1 STARTTLS\r\n");
        }
        assert!(request_starttls(Cursor::new(vec![b'x'; 65537])).is_err());
    }

    async fn session_login(
        endpoint: &ImapEndpoint,
        auth: &ImapAuth,
    ) -> Result<ImapSession, InfraError> {
        super::session_login(endpoint, auth, &MailOperation::new(LOGIN_TIMEOUT)).await
    }

    #[tokio::test]
    async fn rejects_unsupported_modes_before_dns() {
        let endpoint = ImapEndpoint {
            host: "must-not-resolve.invalid".into(),
            port: 993,
            tls: MailTlsMode::Implicit,
        };
        assert!(matches!(
            session_login(
                &endpoint,
                &ImapAuth::OAuth2 {
                    username: "u".into(),
                    access_token: "t".into()
                }
            )
            .await,
            Err(InfraError::Auth { .. })
        ));
        let endpoint = ImapEndpoint {
            tls: MailTlsMode::Plain,
            ..endpoint
        };
        assert!(matches!(
            session_login(&endpoint, &password()).await,
            Err(InfraError::Remote {
                retryable: false,
                ..
            })
        ));
    }

    fn password() -> ImapAuth {
        ImapAuth::Password {
            username: " user ".into(),
            password: " secret ".into(),
        }
    }

    #[test]
    fn password_spaces_are_preserved() {
        struct Stream {
            input: Cursor<Vec<u8>>,
            output: Arc<Mutex<Vec<u8>>>,
        }
        impl Read for Stream {
            fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
                self.input.read(bytes)
            }
        }
        impl Write for Stream {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.output.lock().unwrap().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let output = Arc::new(Mutex::new(Vec::new()));
        let stream = Stream {
            input: Cursor::new(b"a1 OK logged in\r\n".to_vec()),
            output: output.clone(),
        };
        authenticate(imap::Client::new(stream), &password()).unwrap();
        assert_eq!(
            *output.lock().unwrap(),
            b"a1 LOGIN \"user\" \" secret \"\r\n"
        );
    }

    #[test]
    fn tries_next_address_after_connection_refusal() {
        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        let unavailable = closed.local_addr().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        drop(closed);
        // Windows 的本地拒绝连接也可能耗时约两秒；留足首地址预算。
        let connected = connect_tcp(
            &[unavailable, listener.local_addr().unwrap()],
            Instant::now() + Duration::from_secs(8),
            &MailOperation::new(Duration::from_secs(8)),
        )
        .unwrap();
        assert_eq!(
            connected.peer_addr().unwrap(),
            listener.local_addr().unwrap()
        );
    }

    #[tokio::test]
    async fn stalled_tls_and_starttls_are_interrupted_without_sending_credentials() {
        for mode in [MailTlsMode::Implicit, MailTlsMode::StartTls] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = ImapEndpoint {
                host: "127.0.0.1".into(),
                port: listener.local_addr().unwrap().port(),
                tls: mode,
            };
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut bytes = Vec::new();
                let _ = stream.read_to_end(&mut bytes);
                bytes
            });
            let start = Instant::now();
            let result =
                session_login_with_timeout(&endpoint, &password(), Duration::from_millis(250))
                    .await;
            assert!(
                result.as_ref().err().unwrap().to_string().contains("超时"),
                "{result:?}"
            );
            assert!(start.elapsed() < Duration::from_secs(2));
            let bytes = server.join().unwrap();
            assert!(!bytes.windows(6).any(|part| part == b"LOGIN "));
            assert!(!bytes.windows(6).any(|part| part == b"secret"));
        }
    }

    #[tokio::test]
    async fn starttls_uses_same_deadline_after_upgrade_confirmation() {
        use std::io::BufRead;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = ImapEndpoint {
            host: "127.0.0.1".into(),
            port: listener.local_addr().unwrap().port(),
            tls: MailTlsMode::StartTls,
        };
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            stream.write_all(b"* OK ready\r\n").unwrap();
            let mut reader = std::io::BufReader::new(stream);
            let mut command = String::new();
            reader.read_line(&mut command).unwrap();
            assert_eq!(command, "a1 STARTTLS\r\n");
            reader.get_mut().write_all(b"a1 OK upgrade\r\n").unwrap();
            let mut handshake = Vec::new();
            let _ = reader.read_to_end(&mut handshake);
            assert!(!handshake.is_empty());
            assert!(!handshake.windows(6).any(|bytes| bytes == b"LOGIN "));
        });
        let start = Instant::now();
        let result =
            session_login_with_timeout(&endpoint, &password(), Duration::from_millis(500)).await;
        assert!(
            result.as_ref().err().unwrap().to_string().contains("超时"),
            "{result:?}"
        );
        assert!(start.elapsed() < Duration::from_secs(2));
        server.join().unwrap();
    }

    #[tokio::test]
    async fn dropping_login_closes_the_socket() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = ImapEndpoint {
            host: "127.0.0.1".into(),
            port: listener.local_addr().unwrap().port(),
            tls: MailTlsMode::StartTls,
        };
        let (accepted_tx, accepted_rx) = tokio::sync::oneshot::channel();
        let (closed_tx, closed_rx) = tokio::sync::oneshot::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            accepted_tx.send(()).unwrap();
            let result = stream.read(&mut [0; 1]);
            closed_tx.send(result).unwrap();
        });
        let login = tokio::spawn(async move { session_login(&endpoint, &password()).await });
        accepted_rx.await.unwrap();
        login.abort();
        let _ = login.await;
        let result = tokio::time::timeout(Duration::from_secs(2), closed_rx)
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(result, Ok(0))
                || result.is_err_and(|error| error.kind() != std::io::ErrorKind::TimedOut)
        );
        server.join().unwrap();
    }
}
