//! SMTP 发信适配：TLS / STARTTLS、密码、XOAUTH2、回复头、附件。

mod send;

pub use send::{send_message, SmtpAuth, SmtpEndpoint, SmtpOutgoing, SmtpSendResult};

pub async fn load_connection(
    credentials_dir: &std::path::Path,
    config: &crate::domain::entities::PasswordMailConfig,
) -> Result<(SmtpEndpoint, SmtpAuth), crate::infrastructure::error::InfraError> {
    let secret = crate::infrastructure::mail::credentials::load(credentials_dir, &config.auth).await?
        .ok_or_else(|| crate::infrastructure::error::InfraError::Auth { message: "缺少 SMTP 密码或邮箱授权码".into() })?;
    Ok((SmtpEndpoint { host: config.endpoint.host().to_owned(), port: config.endpoint.port(), tls: config.endpoint.tls() },
        SmtpAuth::Password { username: config.auth.username().to_owned(), password: secret.password }))
}
