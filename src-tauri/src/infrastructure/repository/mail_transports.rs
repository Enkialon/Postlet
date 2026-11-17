//! SQLite account_mail_transport 模块函数。

use crate::infrastructure::error::InfraError;
use crate::domain::entities::AccountMailTransport;
use crate::domain::value_objects::{
    AccountId, MailAuthKind, MailTlsMode, ReceiveProtocol, SendProtocol,
};
use sqlx::{Row, SqliteConnection};

fn map_row(row: &sqlx::sqlite::SqliteRow) -> Result<AccountMailTransport, InfraError> {
    let receive = row
        .try_get::<String, _>("receive_protocol")
        .map_err(|_| InfraError::Storage {
            message: "读取 receive_protocol 失败".into(),
        })?;
    let send = row
        .try_get::<String, _>("send_protocol")
        .map_err(|_| InfraError::Storage {
            message: "读取 send_protocol 失败".into(),
        })?;
    let auth = row
        .try_get::<String, _>("auth_kind")
        .map_err(|_| InfraError::Storage {
            message: "读取 auth_kind 失败".into(),
        })?;
    let imap_tls = row
        .try_get::<Option<String>, _>("imap_tls")
        .map_err(|_| InfraError::Storage {
            message: "读取 imap_tls 失败".into(),
        })?;
    let smtp_tls = row
        .try_get::<Option<String>, _>("smtp_tls")
        .map_err(|_| InfraError::Storage {
            message: "读取 smtp_tls 失败".into(),
        })?;
    Ok(AccountMailTransport {
        account_id: AccountId::new(
            row.try_get::<String, _>("account_id")
                .map_err(|_| InfraError::Storage {
                    message: "读取 account_id 失败".into(),
                })?,
        ),
        receive_protocol: ReceiveProtocol::parse(&receive).ok_or_else(|| InfraError::Storage {
            message: format!("无效 receive_protocol: {receive}"),
        })?,
        send_protocol: SendProtocol::parse(&send).ok_or_else(|| InfraError::Storage {
            message: format!("无效 send_protocol: {send}"),
        })?,
        auth_kind: MailAuthKind::parse(&auth).ok_or_else(|| InfraError::Storage {
            message: format!("无效 auth_kind: {auth}"),
        })?,
        credential_ref: row.try_get("credential_ref")?,
        imap_host: row.try_get("imap_host").ok(),
        imap_port: row
            .try_get::<Option<i64>, _>("imap_port")
            .ok()
            .flatten()
            .and_then(|p| u16::try_from(p).ok()),
        imap_tls: imap_tls.as_deref().and_then(MailTlsMode::parse),
        smtp_host: row.try_get("smtp_host").ok(),
        smtp_port: row
            .try_get::<Option<i64>, _>("smtp_port")
            .ok()
            .flatten()
            .and_then(|p| u16::try_from(p).ok()),
        smtp_tls: smtp_tls.as_deref().and_then(MailTlsMode::parse),
        username: row.try_get("username").ok(),
        imap_idle_supported: row
            .try_get::<Option<i64>, _>("imap_idle_supported")
            .ok()
            .flatten()
            .map(|v| v != 0),
        updated_at: row.try_get("updated_at").map_err(|_| InfraError::Storage {
            message: "读取 updated_at 失败".into(),
        })?,
    })
}

pub async fn get(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
) -> Result<Option<AccountMailTransport>, InfraError> {
    let row = sqlx::query("SELECT * FROM account_mail_transport WHERE account_id = ?")
        .bind(account_id.as_str())
        .fetch_optional(&mut *conn)
        .await?;
    row.as_ref().map(map_row).transpose()
}

pub async fn upsert(
    conn: &mut SqliteConnection,
    transport: &AccountMailTransport,
) -> Result<(), InfraError> {
    sqlx::query(
        "INSERT INTO account_mail_transport (
            account_id, receive_protocol, send_protocol, auth_kind,
            imap_host, imap_port, imap_tls, smtp_host, smtp_port, smtp_tls,
            username, imap_idle_supported, updated_at, credential_ref
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(account_id) DO UPDATE SET
            receive_protocol=excluded.receive_protocol,
            send_protocol=excluded.send_protocol,
            auth_kind=excluded.auth_kind,
            credential_ref=excluded.credential_ref,
            imap_host=excluded.imap_host,
            imap_port=excluded.imap_port,
            imap_tls=excluded.imap_tls,
            smtp_host=excluded.smtp_host,
            smtp_port=excluded.smtp_port,
            smtp_tls=excluded.smtp_tls,
            username=excluded.username,
            imap_idle_supported=excluded.imap_idle_supported,
            updated_at=excluded.updated_at",
    )
    .bind(transport.account_id.as_str())
    .bind(transport.receive_protocol.as_str())
    .bind(transport.send_protocol.as_str())
    .bind(transport.auth_kind.as_str())
    .bind(&transport.imap_host)
    .bind(transport.imap_port.map(i64::from))
    .bind(transport.imap_tls.map(|t| t.as_str()))
    .bind(&transport.smtp_host)
    .bind(transport.smtp_port.map(i64::from))
    .bind(transport.smtp_tls.map(|t| t.as_str()))
    .bind(&transport.username)
    .bind(transport.imap_idle_supported.map(|v| if v { 1_i64 } else { 0 }))
    .bind(transport.updated_at)
    .bind(&transport.credential_ref)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// 能力观测只属于探测时的收信配置，不能用旧快照覆盖新端点或凭据。
pub async fn record_imap_idle_support(
    conn: &mut SqliteConnection,
    observed_config: &AccountMailTransport,
    supported: bool,
) -> Result<bool, InfraError> {
    if observed_config.receive_protocol != ReceiveProtocol::Imap { return Ok(false); }
    let result = sqlx::query(
        "UPDATE account_mail_transport SET imap_idle_supported = ?
         WHERE account_id = ? AND receive_protocol = 'imap'
           AND auth_kind = ? AND credential_ref IS ?
           AND imap_host IS ? AND imap_port IS ? AND imap_tls IS ? AND username IS ?
           AND EXISTS (SELECT 1 FROM accounts
                       WHERE id = account_mail_transport.account_id AND is_signed_in = 1)",
    )
    .bind(supported)
    .bind(observed_config.account_id.as_str())
    .bind(observed_config.auth_kind.as_str())
    .bind(&observed_config.credential_ref)
    .bind(&observed_config.imap_host)
    .bind(observed_config.imap_port.map(i64::from))
    .bind(observed_config.imap_tls.map(|tls| tls.as_str()))
    .bind(&observed_config.username)
    .execute(&mut *conn).await?;
    Ok(result.rows_affected() == 1)
}

/// 确保账号有协议行；缺失则写入 Graph 默认。
pub async fn ensure_default(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    now: i64,
) -> Result<AccountMailTransport, InfraError> {
    if let Some(existing) = get(conn, account_id).await? {
        return Ok(existing);
    }
    let transport = AccountMailTransport::graph_default(account_id.clone(), now);
    upsert(conn, &transport).await?;
    Ok(transport)
}
