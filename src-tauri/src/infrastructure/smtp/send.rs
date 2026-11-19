//! SMTP 发送实现（lettre）

use crate::domain::mail::{SendOutcome, SendResult, SendStage};
use crate::domain::value_objects::MailTlsMode;
use crate::infrastructure::error::InfraError;
use lettre::message::{header, Attachment, Body, Mailbox, MessageBuilder, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct SmtpEndpoint {
    pub host: String,
    pub port: u16,
    pub tls: MailTlsMode,
}

#[derive(Clone)]
pub enum SmtpAuth {
    Password { username: String, password: String },
    OAuth2 { username: String, access_token: String },
}

#[derive(Debug, Clone)]
pub struct SmtpAttachment {
    pub filename: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct SmtpOutgoing {
    pub from: String,
    pub from_name: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub text_body: Option<String>,
    pub html_body: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    pub attachments: Vec<SmtpAttachment>,
}

/// 发送结论：阶段 + 业务语义（禁止统一 retryable=true）
pub type SmtpSendResult = SendResult;

fn parse_mailbox(addr: &str, name: &str) -> Result<Mailbox, InfraError> {
    let parsed = if name.trim().is_empty() {
        addr.parse()
    } else {
        format!("{name} <{addr}>").parse()
    };
    parsed.map_err(|e| InfraError::Remote {
        message: format!("无效邮箱地址: {e}"),
        retryable: false,
    })
}

pub async fn send_message(
    endpoint: &SmtpEndpoint,
    auth: &SmtpAuth,
    mail: &SmtpOutgoing,
) -> Result<SmtpSendResult, InfraError> {
    if matches!(auth, SmtpAuth::OAuth2 { .. }) {
        return Err(InfraError::Auth {
            message: "通用 SMTP V1 不支持 OAuth2，请使用密码或邮箱授权码".into(),
        });
    }
    // Plain 在建立任何连接前拒绝：不允许明文认证或明文投递
    if endpoint.tls == MailTlsMode::Plain {
        return Err(InfraError::Remote {
            message: "SMTP 明文连接已禁用，请使用隐式 TLS 或 STARTTLS".into(),
            retryable: false,
        });
    }
    let mut builder = Message::builder()
        .from(parse_mailbox(&mail.from, &mail.from_name)?)
        .subject(mail.subject.clone());
    for to in &mail.to {
        builder = builder.to(parse_mailbox(to, "")?);
    }
    for cc in &mail.cc {
        builder = builder.cc(parse_mailbox(cc, "")?);
    }
    for bcc in &mail.bcc {
        builder = builder.bcc(parse_mailbox(bcc, "")?);
    }
    if let Some(irt) = &mail.in_reply_to {
        builder = builder.header(header::InReplyTo::from(irt.clone()));
    }
    if let Some(refs) = &mail.references {
        builder = builder.header(header::References::from(refs.clone()));
    }

    let message = build_body(builder, mail)?;

    let tls_params = TlsParameters::new(endpoint.host.clone()).map_err(|e| InfraError::Network {
        message: format!("SMTP TLS 参数失败: {e}"),
    })?;

    let builder = match endpoint.tls {
        MailTlsMode::Implicit => AsyncSmtpTransport::<Tokio1Executor>::relay(&endpoint.host)
            .map_err(|e| InfraError::Network {
                message: format!("SMTP relay 失败: {e}"),
            })?
            .port(endpoint.port)
            .tls(Tls::Wrapper(tls_params)),
        MailTlsMode::StartTls => AsyncSmtpTransport::<Tokio1Executor>::relay(&endpoint.host)
            .map_err(|e| InfraError::Network {
                message: format!("SMTP relay 失败: {e}"),
            })?
            .port(endpoint.port)
            .tls(Tls::Required(tls_params)),
        MailTlsMode::Plain => unreachable!("明文在连接前已被拒绝"),
    };

    let transport = match auth {
        SmtpAuth::Password { username, password } => builder
            .credentials(Credentials::new(username.clone(), password.clone()))
            .timeout(Some(Duration::from_secs(60)))
            .build(),
        SmtpAuth::OAuth2 { .. } => unreachable!("OAuth2 rejected before connection"),
    };

    match transport.send(message).await {
        Ok(_) => Ok(SendResult::new(SendOutcome::Accepted, SendStage::Data)),
        Err(err) => classify_smtp_result(&err),
    }
}

fn build_body(builder: MessageBuilder, mail: &SmtpOutgoing) -> Result<Message, InfraError> {
    let text = mail.text_body.clone().unwrap_or_default();
    let html = mail.html_body.clone();

    if mail.attachments.is_empty() {
        if let Some(html) = html {
            let multipart = MultiPart::alternative()
                .singlepart(SinglePart::plain(text))
                .singlepart(SinglePart::html(html));
            return builder.multipart(multipart).map_err(|e| InfraError::Remote {
                message: format!("构造邮件失败: {e}"),
                retryable: false,
            });
        }
        return builder.body(text).map_err(|e| InfraError::Remote {
            message: format!("构造邮件失败: {e}"),
            retryable: false,
        });
    }

    let mut mixed = if let Some(html) = html {
        MultiPart::mixed().multipart(
            MultiPart::alternative()
                .singlepart(SinglePart::plain(text))
                .singlepart(SinglePart::html(html)),
        )
    } else {
        MultiPart::mixed().singlepart(SinglePart::plain(text))
    };

    for att in &mail.attachments {
        let body = Body::new(att.bytes.clone());
        let content_type = att
            .mime
            .parse()
            .unwrap_or_else(|_| "application/octet-stream".parse().expect("mime"));
        let part = Attachment::new(att.filename.clone()).body(body, content_type);
        mixed = mixed.singlepart(part);
    }

    builder.multipart(mixed).map_err(|e| InfraError::Remote {
        message: format!("构造带附件邮件失败: {e}"),
        retryable: false,
    })
}

/// 直接投递已持久化的 MIME 字节；envelope 决定 RCPT（含 Bcc），不再重新构造内容。
pub async fn send_raw_message(
    endpoint: &SmtpEndpoint,
    auth: &SmtpAuth,
    mime: &[u8],
    envelope: &crate::infrastructure::mail::outbound_content::SnapshotEnvelope,
) -> Result<SendResult, InfraError> {
    use lettre::address::Envelope;

    if matches!(auth, SmtpAuth::OAuth2 { .. }) {
        return Err(InfraError::Auth {
            message: "通用 SMTP V1 不支持 OAuth2，请使用密码或邮箱授权码".into(),
        });
    }
    if endpoint.tls == MailTlsMode::Plain {
        return Err(InfraError::Remote {
            message: "SMTP 明文连接已禁用，请使用隐式 TLS 或 STARTTLS".into(),
            retryable: false,
        });
    }

    let to: Vec<lettre::Address> = envelope
        .to
        .iter()
        .chain(envelope.cc.iter())
        .chain(envelope.bcc.iter())
        .filter_map(|r| r.address.parse().ok())
        .collect();
    let from: lettre::Address = envelope.from.parse().map_err(|_| InfraError::Remote {
        message: "无效发件地址".into(),
        retryable: false,
    })?;
    let envelope = Envelope::new(Some(from), to).map_err(|e| InfraError::Remote {
        message: format!("构造投递 envelope 失败: {e}"),
        retryable: false,
    })?;

    let tls_params = TlsParameters::new(endpoint.host.clone()).map_err(|e| InfraError::Network {
        message: format!("SMTP TLS 参数失败: {e}"),
    })?;
    let builder = match endpoint.tls {
        MailTlsMode::Implicit => AsyncSmtpTransport::<Tokio1Executor>::relay(&endpoint.host)
            .map_err(|e| InfraError::Network {
                message: format!("SMTP relay 失败: {e}"),
            })?
            .port(endpoint.port)
            .tls(Tls::Wrapper(tls_params)),
        MailTlsMode::StartTls => AsyncSmtpTransport::<Tokio1Executor>::relay(&endpoint.host)
            .map_err(|e| InfraError::Network {
                message: format!("SMTP relay 失败: {e}"),
            })?
            .port(endpoint.port)
            .tls(Tls::Required(tls_params)),
        MailTlsMode::Plain => unreachable!("明文在连接前已被拒绝"),
    };
    let transport = match auth {
        SmtpAuth::Password { username, password } => builder
            .credentials(Credentials::new(username.clone(), password.clone()))
            .timeout(Some(Duration::from_secs(60)))
            .build(),
        SmtpAuth::OAuth2 { .. } => unreachable!("OAuth2 rejected before connection"),
    };

    // 发送阶段无法从 lettre 错误精确还原：有 SMTP 状态码时按码分类；
    // 无状态码的错误按「能否证明未提交」分类，不猜测已经写入内容的结果。
    match transport.send_raw(&envelope, mime).await {
        Ok(_) => Ok(SendResult::new(SendOutcome::Accepted, SendStage::Data)),
        Err(err) => classify_smtp_result(&err),
    }
}

/// 将 lettre 错误映射为发送结论。
///
/// 与锁定版本 lettre 0.11.23 的 `smtp::Error` API 对应：
/// `status()` 给出服务端应答码（证明内容未被接受）；
/// `is_tls` / `is_client` 是本地即可确定失败；
/// 建连/DNS 失败（`Display` 固定为 `Connection error`）可证明尚未写入；
/// 其余（写入/读应答的 IO、超时、应答解析失败、连接被关闭）无法证明未提交，
/// 只能记为结果不确定，绝不自动重发。
fn classify_smtp_result(
    err: &lettre::transport::smtp::Error,
) -> Result<SendResult, InfraError> {
    let detail = err.to_string();

    if let Some(code) = err.status() {
        let code: u16 = code
            .to_string()
            .split_whitespace()
            .next()
            .and_then(|c| c.parse().ok())
            .unwrap_or(0);
        return match code {
            // 认证类拒绝：530/534/535 等
            530..=539 => Err(InfraError::Auth {
                message: format!("SMTP 认证被拒绝 ({code}): {detail}"),
            }),
            // 4xx：服务端明确暂时拒绝，可退避重试
            400..=499 => Err(InfraError::Remote {
                message: format!("SMTP 暂时拒绝 ({code}): {detail}"),
                retryable: true,
            }),
            // 5xx：永久拒绝
            _ => Err(InfraError::Remote {
                message: format!("SMTP 永久拒绝 ({code}): {detail}"),
                retryable: false,
            }),
        };
    }

    if err.is_tls() {
        // TLS 不可用：重试无意义，且绝不能降级为明文
        return Err(InfraError::Remote {
            message: format!("SMTP 无法建立受保护连接: {detail}"),
            retryable: false,
        });
    }
    if err.is_client() {
        return Err(InfraError::Remote {
            message: format!("SMTP 客户端拒绝发送: {detail}"),
            retryable: false,
        });
    }
    if is_connection_failure(err) {
        // 建连/DNS 失败：可证明未提交，按可恢复的暂时失败处理。
        return Ok(SendResult::new(SendOutcome::Transient, SendStage::Connect));
    }

    Ok(SendResult::new(SendOutcome::Uncertain, SendStage::Data))
}

/// lettre 0.11 未公开连接阶段错误种类；建连/DNS 失败的 `Display` 固定以
/// `Connection error` 开头，且不会在会话中途产生。
fn is_connection_failure(err: &lettre::transport::smtp::Error) -> bool {
    err.to_string().starts_with("Connection error")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unsupported_oauth_is_rejected_before_building_or_connecting() {
        let mail = SmtpOutgoing {
            from: String::new(), from_name: String::new(), to: vec![], cc: vec![], bcc: vec![],
            subject: String::new(), text_body: None, html_body: None,
            in_reply_to: None, references: None, attachments: vec![],
        };
        let error = send_message(
            &SmtpEndpoint { host: "must-not-resolve.invalid".into(), port: 587, tls: MailTlsMode::StartTls },
            &SmtpAuth::OAuth2 { username: "user".into(), access_token: "token".into() },
            &mail,
        ).await.unwrap_err();
        assert!(matches!(error, InfraError::Auth { .. }));
        assert!(error.to_string().contains("不支持 OAuth2"));
    }

    /// 明文路径必须在建立连接前拒绝：用不可解析主机证明零网络访问。
    #[tokio::test]
    async fn plain_tls_is_rejected_before_any_connection() {
        let mail = SmtpOutgoing {
            from: "me@example.com".into(),
            from_name: "我".into(),
            to: vec!["to@example.com".into()],
            cc: vec![],
            bcc: vec![],
            subject: "s".into(),
            text_body: Some("t".into()),
            html_body: None,
            in_reply_to: None,
            references: None,
            attachments: vec![],
        };
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            send_message(
                &SmtpEndpoint {
                    host: "plain-must-not-connect.invalid".into(),
                    port: 25,
                    tls: MailTlsMode::Plain,
                },
                &SmtpAuth::Password {
                    username: "u".into(),
                    password: "p".into(),
                },
                &mail,
            ),
        )
        .await
        .expect("明文拒绝必须立即返回，不得尝试连接")
        .unwrap_err();
        assert!(matches!(error_kind(&result), Some(false)), "明文拒绝不可重试");
        assert!(result.to_string().contains("明文连接已禁用"));
    }

    fn error_kind(err: &InfraError) -> Option<bool> {
        match err {
            InfraError::Remote { retryable, .. } => Some(*retryable),
            _ => None,
        }
    }

    fn envelope() -> crate::infrastructure::mail::outbound_content::SnapshotEnvelope {
        use crate::infrastructure::mail::outbound_content::{SnapshotEnvelope, SnapshotRecipient};
        SnapshotEnvelope {
            from: "me@example.com".into(),
            from_name: String::new(),
            to: vec![SnapshotRecipient {
                address: "to@example.com".into(),
                name: String::new(),
            }],
            cc: vec![],
            bcc: vec![],
        }
    }

    /// 建连失败可证明未提交：既不能记为结果未知，也不能直接判死。
    #[tokio::test]
    async fn connect_failure_is_recoverable_instead_of_unknown_or_failed() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            send_raw_message(
                &SmtpEndpoint {
                    host: "127.0.0.1".into(),
                    port,
                    tls: MailTlsMode::Implicit,
                },
                &SmtpAuth::Password {
                    username: "u".into(),
                    password: "p".into(),
                },
                b"From: me@example.com\r\n\r\nbody",
                &envelope(),
            ),
        )
        .await
        .expect("建连失败必须立即返回，不得等待超时")
        .unwrap();
        assert_eq!(
            result,
            SendResult::new(SendOutcome::Transient, SendStage::Connect)
        );
    }

    /// 本地即可确定的失败仍是确定失败，不会退化为未知。
    #[tokio::test]
    async fn invalid_envelope_is_definitely_not_sent() {
        let mut envelope = envelope();
        envelope.from = "not-an-address".into();
        let error = send_raw_message(
            &SmtpEndpoint {
                host: "must-not-connect.invalid".into(),
                port: 587,
                tls: MailTlsMode::Implicit,
            },
            &SmtpAuth::Password {
                username: "u".into(),
                password: "p".into(),
            },
            b"body",
            &envelope,
        )
        .await
        .unwrap_err();
        assert!(matches!(error_kind(&error), Some(false)));
        assert!(error.to_string().contains("无效发件地址"));
    }
}
