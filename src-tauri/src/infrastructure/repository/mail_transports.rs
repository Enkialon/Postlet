//! SQLite account_mail_transport；存储结构校验后才能构造收发配置。
use crate::infrastructure::error::InfraError;
use crate::domain::entities::{AccountMailTransport, ReceiveConfig, SendConfig, GraphAuthConfig, PasswordMailConfig, PasswordAuthConfig, MailEndpoint};
use crate::domain::value_objects::{AccountId, CredentialRef, MailTlsMode};
use sqlx::{Row, SqliteConnection};

fn invalid() -> InfraError { InfraError::Storage { message: "邮件协议配置无效，请重新配置账号".into() } }
fn password_config(row: &sqlx::sqlite::SqliteRow, host: &str, port: &str, tls: &str, username: &str, reference: CredentialRef) -> Result<PasswordMailConfig, InfraError> {
    let port = u16::try_from(row.try_get::<i64,_>(port)?).map_err(|_| invalid())?;
    let tls = MailTlsMode::parse(&row.try_get::<String,_>(tls)?).ok_or_else(invalid)?;
    Ok(PasswordMailConfig {
        endpoint: MailEndpoint::new(row.try_get(host)?, port, tls).map_err(|_| invalid())?,
        auth: PasswordAuthConfig::new(row.try_get(username)?, reference).map_err(|_| invalid())?,
    })
}
fn map_row(row: &sqlx::sqlite::SqliteRow) -> Result<AccountMailTransport, InfraError> {
    let receive_ref = CredentialRef::new(row.try_get::<String,_>("receive_credential_ref")?).map_err(|_| invalid())?;
    let send_ref = CredentialRef::new(row.try_get::<String,_>("send_credential_ref")?).map_err(|_| invalid())?;
    let receive = match row.try_get::<String,_>("receive_protocol")?.as_str() {
        "graph" => ReceiveConfig::Graph(GraphAuthConfig { credential_ref: receive_ref }),
        "imap" => ReceiveConfig::Imap(password_config(row, "imap_host", "imap_port", "imap_tls", "imap_username", receive_ref)?),
        _ => return Err(invalid()),
    };
    let send = match row.try_get::<String,_>("send_protocol")?.as_str() {
        "graph" => SendConfig::Graph(GraphAuthConfig { credential_ref: send_ref }),
        "smtp" => SendConfig::Smtp(password_config(row, "smtp_host", "smtp_port", "smtp_tls", "smtp_username", send_ref)?),
        _ => return Err(invalid()),
    };
    Ok(AccountMailTransport { account_id: AccountId::new(row.try_get::<String,_>("account_id")?), receive, send,
        imap_idle_supported: row.try_get("imap_idle_supported")?, updated_at: row.try_get("updated_at")? })
}

pub async fn get(conn: &mut SqliteConnection, account_id: &AccountId) -> Result<Option<AccountMailTransport>, InfraError> {
    let row = sqlx::query("SELECT * FROM account_mail_transport WHERE account_id = ?").bind(account_id.as_str()).fetch_optional(&mut *conn).await?;
    row.as_ref().map(map_row).transpose()
}

pub async fn upsert(conn: &mut SqliteConnection, transport: &AccountMailTransport) -> Result<(), InfraError> {
    let receive = match &transport.receive { ReceiveConfig::Graph(_) => None, ReceiveConfig::Imap(config) => Some(config) };
    let send = match &transport.send { SendConfig::Graph(_) => None, SendConfig::Smtp(config) => Some(config) };
    sqlx::query("INSERT INTO account_mail_transport (
        account_id, receive_protocol, send_protocol, receive_credential_ref, send_credential_ref,
        imap_host, imap_port, imap_tls, imap_username, smtp_host, smtp_port, smtp_tls, smtp_username, imap_idle_supported, updated_at
    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    ON CONFLICT(account_id) DO UPDATE SET receive_protocol=excluded.receive_protocol, send_protocol=excluded.send_protocol,
        receive_credential_ref=excluded.receive_credential_ref, send_credential_ref=excluded.send_credential_ref,
        imap_host=excluded.imap_host, imap_port=excluded.imap_port, imap_tls=excluded.imap_tls, imap_username=excluded.imap_username,
        smtp_host=excluded.smtp_host, smtp_port=excluded.smtp_port, smtp_tls=excluded.smtp_tls, smtp_username=excluded.smtp_username,
        imap_idle_supported=excluded.imap_idle_supported, updated_at=excluded.updated_at")
        .bind(transport.account_id.as_str()).bind(transport.receive.protocol().as_str()).bind(transport.send.protocol().as_str())
        .bind(transport.receive.credential_ref().as_str()).bind(transport.send.credential_ref().as_str())
        .bind(receive.map(|config| config.endpoint.host())).bind(receive.map(|config| i64::from(config.endpoint.port())))
        .bind(receive.map(|config| config.endpoint.tls().as_str())).bind(receive.map(|config| config.auth.username()))
        .bind(send.map(|config| config.endpoint.host())).bind(send.map(|config| i64::from(config.endpoint.port())))
        .bind(send.map(|config| config.endpoint.tls().as_str())).bind(send.map(|config| config.auth.username()))
        .bind(transport.imap_idle_supported).bind(transport.updated_at).execute(&mut *conn).await?;
    Ok(())
}

pub async fn record_imap_idle_support(conn: &mut SqliteConnection, observed: &AccountMailTransport, supported: bool) -> Result<bool, InfraError> {
    let ReceiveConfig::Imap(config) = &observed.receive else { return Ok(false) };
    Ok(sqlx::query("UPDATE account_mail_transport SET imap_idle_supported = ? WHERE account_id = ? AND receive_protocol = 'imap'
        AND receive_credential_ref = ? AND imap_host = ? AND imap_port = ? AND imap_tls = ? AND imap_username = ?
        AND EXISTS (SELECT 1 FROM accounts WHERE id = account_mail_transport.account_id AND is_signed_in = 1)")
        .bind(supported).bind(observed.account_id.as_str()).bind(config.auth.credential_ref().as_str())
        .bind(config.endpoint.host()).bind(i64::from(config.endpoint.port())).bind(config.endpoint.tls().as_str()).bind(config.auth.username())
        .execute(&mut *conn).await?.rows_affected() == 1)
}

pub async fn require(conn: &mut SqliteConnection, account_id: &AccountId) -> Result<AccountMailTransport, InfraError> {
    get(conn, account_id).await?.ok_or_else(|| InfraError::Conflict { message: "账号邮件协议配置缺失，请重新配置账号".into() })
}

pub async fn is_credential_referenced(conn: &mut SqliteConnection, reference: &CredentialRef) -> Result<bool, InfraError> {
    Ok(sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM account_mail_transport WHERE receive_credential_ref = ? OR send_credential_ref = ?)")
        .bind(reference.as_str()).bind(reference.as_str()).fetch_one(&mut *conn).await?)
}
