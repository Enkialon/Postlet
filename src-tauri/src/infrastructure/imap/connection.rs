//! 有界连接：DNS、地址回退、TLS、问候与认证共用一个截止时间。

use super::client::{ImapAuth, ImapEndpoint, ImapSession};
use crate::domain::value_objects::MailTlsMode;
use crate::infrastructure::error::InfraError;
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const LOGIN_TIMEOUT: Duration = Duration::from_secs(60);
const ADDRESS_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Default)]
struct SocketState {
    cancelled: bool,
    socket: Option<TcpStream>,
}

/// Future 被取消时也关闭底层 socket，不能留下继续认证的阻塞任务。
struct LoginGuard(Arc<Mutex<SocketState>>);

impl Drop for LoginGuard {
    fn drop(&mut self) {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        state.cancelled = true;
        if let Some(socket) = state.socket.take() {
            let _ = socket.shutdown(Shutdown::Both);
        }
    }
}

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
    state: &Mutex<SocketState>,
) -> Result<TcpStream, InfraError> {
    for address in addresses {
        if state.lock().unwrap_or_else(|e| e.into_inner()).cancelled {
            return Err(network("IMAP 连接已取消"));
        }
        let timeout = remaining(deadline)?.min(ADDRESS_TIMEOUT);
        let tcp = match TcpStream::connect_timeout(address, timeout) {
            Ok(tcp) => tcp,
            Err(_) => continue,
        };
        let timeout = remaining(deadline)?;
        tcp.set_read_timeout(Some(timeout))
            .map_err(|_| network("无法设置 IMAP 读取超时"))?;
        tcp.set_write_timeout(Some(timeout))
            .map_err(|_| network("无法设置 IMAP 写入超时"))?;
        let mut guard = state.lock().unwrap_or_else(|e| e.into_inner());
        if guard.cancelled {
            let _ = tcp.shutdown(Shutdown::Both);
            return Err(network("IMAP 连接已取消"));
        }
        guard.socket = Some(tcp.try_clone().map_err(|_| network("无法监控 IMAP 连接"))?);
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

fn login_resolved(
    endpoint: ImapEndpoint,
    auth: ImapAuth,
    addresses: Vec<SocketAddr>,
    deadline: Instant,
    state: Arc<Mutex<SocketState>>,
) -> Result<ImapSession, InfraError> {
    let tcp = connect_tcp(&addresses, deadline, &state)?;
    let tls = native_tls::TlsConnector::builder()
        .build()
        .map_err(|_| network("IMAP TLS 初始化失败"))?;
    let client = if endpoint.tls == MailTlsMode::StartTls {
        // STARTTLS 完成前只交换问候和升级命令，绝不发送认证数据。
        // secure 复用同一个已配置超时的 socket；升级后没有第二次问候。
        let mut probe = imap::Client::new(tcp);
        probe
            .read_greeting()
            .map_err(|_| network("IMAP 问候失败"))?;
        probe
            .secure(&endpoint.host, &tls)
            .map_err(|error| network(format!("IMAP STARTTLS 升级失败: {error}")))?
    } else {
        let stream = tls
            .connect(&endpoint.host, tcp)
            .map_err(|_| network("IMAP TLS 握手失败"))?;
        let mut client = imap::Client::new(stream);
        client
            .read_greeting()
            .map_err(|_| network("IMAP 问候失败"))?;
        client
    };
    authenticate(client, &auth)
}

pub async fn session_login(
    endpoint: &ImapEndpoint,
    auth: &ImapAuth,
) -> Result<ImapSession, InfraError> {
    session_login_with_timeout(endpoint, auth, LOGIN_TIMEOUT).await
}

async fn session_login_with_timeout(
    endpoint: &ImapEndpoint,
    auth: &ImapAuth,
    budget: Duration,
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
    let deadline = Instant::now() + budget;
    let addresses = tokio::time::timeout(
        budget,
        tokio::net::lookup_host((endpoint.host.as_str(), endpoint.port)),
    )
    .await
    .map_err(|_| network("IMAP DNS 解析超时"))?
    .map_err(|_| network("IMAP DNS 解析失败"))?
    .collect::<Vec<_>>();
    let guard = LoginGuard(Arc::new(Mutex::new(SocketState::default())));
    let endpoint = endpoint.clone();
    let auth = auth.clone();
    let state = guard.0.clone();
    let mut worker = tokio::task::spawn_blocking(move || {
        login_resolved(endpoint, auth, addresses, deadline, state)
    });
    let result = tokio::select! {
        result = &mut worker => result.map_err(|_| network("IMAP 登录任务失败"))?,
        _ = tokio::time::sleep(remaining(deadline).unwrap_or(Duration::ZERO)) => {
            drop(guard); // 中断 TLS/问候/认证读取，再等待实际线程退出。
            let _ = worker.await;
            return Err(network("IMAP 连接或认证超时"));
        }
    };
    // 成功返回时移除监控副本，Drop 不关闭交给调用方的会话。
    guard
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .socket
        .take();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read, Write};
    use std::net::TcpListener;

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
            &Mutex::new(SocketState::default()),
        )
        .unwrap();
        assert_eq!(
            connected.peer_addr().unwrap(),
            listener.local_addr().unwrap()
        );
        assert!(connected.read_timeout().unwrap().is_some());
        assert!(connected.write_timeout().unwrap().is_some());
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
