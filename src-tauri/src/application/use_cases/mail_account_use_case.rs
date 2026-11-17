//! 通用邮箱账号：配置 IMAP/SMTP 协议并保存凭证（与 Graph OAuth 登录并列）。

use crate::application::dto::UpsertImapSmtpAccountCommand;
use crate::application::error::AppError;
use crate::domain::entities::{Account, AccountMailTransport};
use crate::domain::value_objects::{
    AccountId, EmailAddress, MailAuthKind, MailTlsMode, ReceiveProtocol, SendProtocol, SyncStatus,
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
    validate(&cmd)?;
    let _lock = credentials::lock(&paths.credentials_dir)?;
    let credential_ref = Uuid::new_v4().to_string();
    let secret = MailSecret {
        username: cmd
            .username
            .clone()
            .unwrap_or_else(|| cmd.email.trim().to_string()),
        password: cmd.password.clone(),
        access_token: None,
        refresh_token: None,
        expires_at_ms: None,
    };
    credentials::save(&paths.credentials_dir, &credential_ref, &secret).await?;
    let result = persist_account(db, cmd, &credential_ref).await;
    match result {
        Ok((account, old_ref)) => {
            if let Some(old_ref) = old_ref {
                credentials::delete(&paths.credentials_dir, &old_ref)
                    .await
                    .map_err(|_| {
                        AppError::internal("账号配置已保存，但旧凭据回收失败；重启时会再次清理")
                    })?;
            }
            Ok(account)
        }
        Err(error) => {
            // commit 返回错误时先确认引用，不能误删可能已经提交的凭据。
            let referenced = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM account_mail_transport WHERE credential_ref = ?",
            )
            .bind(&credential_ref)
            .fetch_one(db)
            .await;
            if matches!(referenced, Ok(0)) {
                credentials::delete(&paths.credentials_dir, &credential_ref)
                    .await
                    .map_err(|_| {
                        AppError::internal("账号配置未完成，新凭据清理失败；重启时会再次清理")
                    })?;
            }
            Err(error)
        }
    }
}

pub(crate) fn validate(cmd: &UpsertImapSmtpAccountCommand) -> Result<(), AppError> {
    if cmd.auth_kind != MailAuthKind::Password {
        return Err(AppError::validation(
            "通用 IMAP/SMTP V1 不支持 OAuth2，请使用密码或邮箱授权码",
        ));
    }
    if EmailAddress::parse(&cmd.email).is_none() {
        return Err(AppError::validation("邮箱地址无效"));
    }
    if cmd.password.as_ref().is_none_or(|value| value.is_empty()) {
        return Err(AppError::validation("密码认证须提供非空 password"));
    }
    for (host, port, tls) in [
        (&cmd.imap_host, cmd.imap_port, cmd.imap_tls),
        (&cmd.smtp_host, cmd.smtp_port, cmd.smtp_tls),
    ] {
        if host.trim() != host
            || host.is_empty()
            || url::Host::parse(host).is_err()
            || host.contains(['/', '@', '?', '#'])
            || port == 0
        {
            return Err(AppError::validation("邮件服务器主机或端口无效"));
        }
        if tls == MailTlsMode::Plain {
            return Err(AppError::validation("邮件服务器必须使用 TLS 或 STARTTLS"));
        }
    }
    if cmd
        .username
        .as_ref()
        .is_some_and(|value| value.trim().is_empty() || value.chars().any(char::is_control))
    {
        return Err(AppError::validation("登录用户名无效"));
    }
    Ok(())
}

async fn persist_account(
    db: &SqlitePool,
    cmd: UpsertImapSmtpAccountCommand,
    credential_ref: &str,
) -> Result<(Account, Option<String>), AppError> {
    let email =
        EmailAddress::parse(&cmd.email).ok_or_else(|| AppError::validation("邮箱地址无效"))?;
    let now = now_ms();
    let mut tx = db.begin().await?;

    let existing = accounts::get_by_email(&mut tx, &email)
        .await
        .map_err(AppError::from)?;
    let previous_transport = if let Some(account) = &existing {
        mail_transports::get(&mut tx, &account.id).await?
    } else { None };
    let old_ref = previous_transport.as_ref().and_then(|transport| transport.credential_ref.clone());
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
        credential_ref: Some(credential_ref.into()),
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
    if previous_transport.as_ref().is_some_and(|old| old.receive_protocol != transport.receive_protocol
        || old.imap_host != transport.imap_host || old.imap_port != transport.imap_port
        || old.imap_tls != transport.imap_tls || old.username != transport.username) {
        let folders = crate::infrastructure::repository::folder_states::list_by_account(&mut tx, &account.id).await?;
        for folder in folders {
            crate::infrastructure::repository::folder_states::retire(&mut tx, &account.id, &folder.folder_id, now).await?;
            let affected = sqlx::query_as::<_, (String, String)>("SELECT id, session_id FROM messages WHERE account_id = ? AND folder_id = ?")
                .bind(account.id.as_str()).bind(folder.folder_id.as_str()).fetch_all(&mut *tx).await?;
            for (mid, sid) in affected {
                let mid = crate::domain::value_objects::MessageId::new(mid);
                crate::infrastructure::repository::messages::set_location_status(&mut tx, &account.id, &mid, crate::domain::mail::LocationStatus::Invalidated).await?;
                crate::infrastructure::repository::mutations::stop_for_message(&mut tx, &account.id, &mid, "收信身份或端点已改变，旧操作已停止").await?;
                crate::infrastructure::repository::sessions::recompute_projection(&mut tx, &account.id, &crate::domain::value_objects::SessionId::new(sid)).await?;
            }
        }
    }
    mail_transports::upsert(&mut tx, &transport)
        .await
        .map_err(AppError::from)?;
    tx.commit().await?;

    Ok((account, old_ref))
}

/// 启动恢复：仅删除没有配置引用的暂存/旧通用凭据；与配置切换互斥。
pub async fn recover_credentials(db: &SqlitePool, paths: &AppPaths) -> Result<(), AppError> {
    let _lock = credentials::lock(&paths.credentials_dir)?;
    let references = sqlx::query_scalar::<_, String>(
        "SELECT credential_ref FROM account_mail_transport WHERE credential_ref IS NOT NULL",
    )
    .fetch_all(db)
    .await?;
    for reference in crate::infrastructure::secret_store::list_ids(&paths.credentials_dir, "mail")?
    {
        if !references.contains(&reference) {
            credentials::delete(&paths.credentials_dir, &reference).await?;
        }
    }
    Ok(())
}
