//! SMTP 发送实现（lettre）

use crate::application::error::PortError;
use crate::domain::value_objects::MailTlsMode;
use lettre::message::{header, Attachment, Body, Mailbox, MessageBuilder, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
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

#[derive(Debug, Clone)]
pub struct SmtpSendResult {
    pub accepted: bool,
}

fn parse_mailbox(addr: &str, name: &str) -> Result<Mailbox, PortError> {
    let parsed = if name.trim().is_empty() {
        addr.parse()
    } else {
        format!("{name} <{addr}>").parse()
    };
    parsed.map_err(|e| PortError::Remote {
        message: format!("无效邮箱地址: {e}"),
        retryable: false,
    })
}

pub async fn send_message(
    endpoint: &SmtpEndpoint,
    auth: &SmtpAuth,
    mail: &SmtpOutgoing,
) -> Result<SmtpSendResult, PortError> {
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

    let tls_params = TlsParameters::new(endpoint.host.clone()).map_err(|e| PortError::Network {
        message: format!("SMTP TLS 参数失败: {e}"),
    })?;

    let builder = match endpoint.tls {
        MailTlsMode::Implicit => AsyncSmtpTransport::<Tokio1Executor>::relay(&endpoint.host)
            .map_err(|e| PortError::Network {
                message: format!("SMTP relay 失败: {e}"),
            })?
            .port(endpoint.port)
            .tls(Tls::Wrapper(tls_params)),
        MailTlsMode::StartTls => AsyncSmtpTransport::<Tokio1Executor>::relay(&endpoint.host)
            .map_err(|e| PortError::Network {
                message: format!("SMTP relay 失败: {e}"),
            })?
            .port(endpoint.port)
            .tls(Tls::Required(tls_params)),
        MailTlsMode::Plain => {
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&endpoint.host)
                .port(endpoint.port)
                .tls(Tls::None)
        }
    };

    let transport = match auth {
        SmtpAuth::Password { username, password } => builder
            .credentials(Credentials::new(username.clone(), password.clone()))
            .timeout(Some(Duration::from_secs(60)))
            .build(),
        SmtpAuth::OAuth2 {
            username,
            access_token,
        } => builder
            .credentials(Credentials::new(username.clone(), access_token.clone()))
            .authentication(vec![Mechanism::Xoauth2])
            .timeout(Some(Duration::from_secs(60)))
            .build(),
    };

    transport
        .send(message)
        .await
        .map_err(|e| PortError::Remote {
            message: format!("SMTP 发送失败: {e}"),
            retryable: true,
        })?;

    Ok(SmtpSendResult { accepted: true })
}

fn build_body(builder: MessageBuilder, mail: &SmtpOutgoing) -> Result<Message, PortError> {
    let text = mail.text_body.clone().unwrap_or_default();
    let html = mail.html_body.clone();

    if mail.attachments.is_empty() {
        if let Some(html) = html {
            let multipart = MultiPart::alternative()
                .singlepart(SinglePart::plain(text))
                .singlepart(SinglePart::html(html));
            return builder.multipart(multipart).map_err(|e| PortError::Remote {
                message: format!("构造邮件失败: {e}"),
                retryable: false,
            });
        }
        return builder.body(text).map_err(|e| PortError::Remote {
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

    builder.multipart(mixed).map_err(|e| PortError::Remote {
        message: format!("构造带附件邮件失败: {e}"),
        retryable: false,
    })
}
