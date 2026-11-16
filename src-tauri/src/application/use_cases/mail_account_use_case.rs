//! 通用邮箱账号：配置 IMAP/SMTP 协议并保存凭证（与 Graph OAuth 登录并列）。

use crate::application::dto::UpsertImapSmtpAccountCommand;
use crate::application::error::AppError;
use crate::domain::entities::{Account, AccountMailTransport};
use crate::domain::value_objects::{
    AccountId, EmailAddress, MailAuthKind, ReceiveProtocol, SendProtocol, SyncStatus,
};
use crate::infrastructure::mail::credentials::{self, MailSecret};
use crate::infrastructure::paths::AppPaths;
use crate::infrastructure::repository::{accounts, mail_transports};
use sqlx::SqlitePool;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 新增或更新 IMAP+SMTP 账号；按 email 复用既有 AccountId。
pub async fn upsert_imap_smtp_account(
    db: &SqlitePool,
    paths: &AppPaths,
    cmd: UpsertImapSmtpAccountCommand,
) -> Result<Account, AppError> {
    let email = EmailAddress::parse(cmd.email.as_str())
        .ok_or_else(|| AppError::validation("邮箱地址无效"))?;
    let now = now_ms();
    let mut tx = db.begin().await?;

    let existing = accounts::get_by_email(&mut tx, &email)
        .await
        .map_err(AppError::from)?;
    let account = if let Some(mut acc) = existing {
        acc.display_name = if cmd.display_name.trim().is_empty() {
            email.as_str().to_string()
        } else {
            cmd.display_name.clone()
        };
        acc.updated_at = now;
        accounts::upsert_identity(&mut tx, &acc)
            .await
            .map_err(AppError::from)?;
        accounts::set_signed_in(&mut tx, &acc.id, true)
            .await
            .map_err(AppError::from)?;
        acc
    } else {
        let account = Account {
            id: AccountId::new(Uuid::new_v4().to_string()),
            email: email.clone(),
            display_name: if cmd.display_name.trim().is_empty() {
                email.as_str().to_string()
            } else {
                cmd.display_name.clone()
            },
            aliases: Vec::new(),
            tenant_id: None,
            home_account_id: None,
            contact_delta_link: None,
            mail_sync_triggered_at: None,
            contact_synced_at: None,
            mail_sync_status: SyncStatus::Idle,
            mail_sync_error: None,
            show_group_messages_in_contact_conversation: false,
            include_in_all: true,
            created_at: now,
            updated_at: now,
        };
        accounts::upsert(&mut tx, &account)
            .await
            .map_err(AppError::from)?;
        account
    };

    let transport = AccountMailTransport {
        account_id: account.id.clone(),
        receive_protocol: ReceiveProtocol::Imap,
        send_protocol: SendProtocol::Smtp,
        auth_kind: cmd.auth_kind,
        imap_host: Some(cmd.imap_host),
        imap_port: Some(cmd.imap_port),
        imap_tls: Some(cmd.imap_tls),
        smtp_host: Some(cmd.smtp_host),
        smtp_port: Some(cmd.smtp_port),
        smtp_tls: Some(cmd.smtp_tls),
        username: Some(cmd.username.unwrap_or_else(|| email.as_str().to_string())),
        imap_idle_supported: None,
        updated_at: now,
    };
    mail_transports::upsert(&mut tx, &transport)
        .await
        .map_err(AppError::from)?;
    tx.commit().await?;

    let secret = match cmd.auth_kind {
        MailAuthKind::Password => MailSecret {
            username: transport.username.clone().unwrap_or_default(),
            password: Some(
                cmd.password
                    .ok_or_else(|| AppError::validation("密码认证须提供 password"))?,
            ),
            access_token: None,
            refresh_token: None,
            expires_at_ms: None,
        },
        MailAuthKind::OAuth2 => MailSecret {
            username: transport.username.clone().unwrap_or_default(),
            password: None,
            access_token: Some(
                cmd.access_token
                    .ok_or_else(|| AppError::validation("OAuth2 须提供 access_token"))?,
            ),
            refresh_token: cmd.refresh_token,
            expires_at_ms: cmd.expires_at_ms,
        },
    };
    credentials::save(&paths.credentials_dir, &account.id, &secret)
        .await
        .map_err(AppError::from)?;

    let mut conn = db.acquire().await?;
    accounts::get(&mut conn, &account.id)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::internal("写入后读取账户失败"))
}
