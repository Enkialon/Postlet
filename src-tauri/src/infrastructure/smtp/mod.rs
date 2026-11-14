//! SMTP 发信适配：TLS / STARTTLS、密码、XOAUTH2、回复头、附件。

mod send;

pub use send::{send_message, SmtpAuth, SmtpEndpoint, SmtpOutgoing, SmtpSendResult};
