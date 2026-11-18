//! 通用邮箱账号：配置 IMAP/SMTP 协议并保存凭证（与 Graph OAuth 登录并列）。

use crate::application::dto::{UpsertImapSmtpAccountCommand, ImapAuthenticationInput, SmtpAuthenticationInput};
use crate::application::error::AppError;
use crate::domain::entities::{Account, AccountMailTransport, ReceiveConfig, SendConfig, MailEndpoint, PasswordAuthConfig, PasswordMailConfig};
use crate::domain::value_objects::{AccountId, EmailAddress, CredentialRef, SyncStatus};
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
    let receive_ref = CredentialRef::new(Uuid::new_v4().to_string())?;
    let send_ref = match cmd.smtp_auth { SmtpAuthenticationInput::Shared => receive_ref.clone(), _ => CredentialRef::new(Uuid::new_v4().to_string())? };
    let ImapAuthenticationInput::Password { username, password } = &cmd.imap_auth;
    let receive = ReceiveConfig::Imap(PasswordMailConfig {
        endpoint: MailEndpoint::new(cmd.imap_host.clone(), cmd.imap_port, cmd.imap_tls)?,
        auth: PasswordAuthConfig::new(username.clone(), receive_ref.clone())?,
    });
    let (send_username, send_password) = match &cmd.smtp_auth {
        SmtpAuthenticationInput::Shared => (username, password),
        SmtpAuthenticationInput::Password { username, password } => (username, password),
    };
    let send = SendConfig::Smtp(PasswordMailConfig {
        endpoint: MailEndpoint::new(cmd.smtp_host.clone(), cmd.smtp_port, cmd.smtp_tls)?,
        auth: PasswordAuthConfig::new(send_username.clone(), send_ref.clone())?,
    });
    let staged = async {
        credentials::save(&paths.credentials_dir, &receive_ref, &MailSecret { password: password.clone() }).await?;
        if send_ref != receive_ref { credentials::save(&paths.credentials_dir, &send_ref, &MailSecret { password: send_password.clone() }).await?; }
        Ok::<_, crate::infrastructure::error::InfraError>(())
    }.await;
    if let Err(error) = staged {
        for reference in [&receive_ref, &send_ref] { credentials::delete(&paths.credentials_dir, reference).await?; }
        return Err(error.into());
    }
    let result = persist_account(db, cmd, receive, send).await;
    match result {
        Ok((account, previous)) => {
            if let Some(previous) = previous {
                credentials::delete_for_transport(&paths.credentials_dir, &previous)
                    .await
                    .map_err(|_| {
                        AppError::internal("账号配置已保存，但旧凭据回收失败；重启时会再次清理")
                    })?;
            }
            Ok(account)
        }
        Err(error) => {
            // 不确定的 COMMIT 先核对两侧引用，不能误删已提交的凭据。
            for reference in [&receive_ref, &send_ref] {
                let referenced = match db.acquire().await {
                    Ok(mut conn) => mail_transports::is_credential_referenced(&mut conn, reference).await,
                    Err(error) => Err(error.into()),
                };
                if matches!(referenced, Ok(false)) { credentials::delete(&paths.credentials_dir, reference).await
                    .map_err(|_| AppError::internal("账号配置未完成，新凭据清理失败；重启时会再次清理"))?; }
            }
            Err(error)
        }
    }
}

pub(crate) fn validate(cmd: &UpsertImapSmtpAccountCommand) -> Result<(), AppError> {
    if EmailAddress::parse(&cmd.email).is_none() { return Err(AppError::validation("邮箱地址无效")); }
    for (host, port, tls) in [(&cmd.imap_host, cmd.imap_port, cmd.imap_tls), (&cmd.smtp_host, cmd.smtp_port, cmd.smtp_tls)] {
        MailEndpoint::new(host.clone(), port, tls).map_err(|error| AppError::validation(error.to_string()))?;
    }
    let ImapAuthenticationInput::Password { username, password } = &cmd.imap_auth;
    let mut inputs = vec![(username, password)];
    if let SmtpAuthenticationInput::Password { username, password } = &cmd.smtp_auth { inputs.push((username,password)); }
    for (username,password) in inputs {
        PasswordAuthConfig::validate_username(username).map_err(|error| AppError::validation(error.to_string()))?;
        if password.is_empty() { return Err(AppError::validation("密码或邮箱授权码不能为空")); }
    }
    Ok(())
}

async fn persist_account(
    db: &SqlitePool,
    cmd: UpsertImapSmtpAccountCommand,
    receive: ReceiveConfig,
    send: SendConfig,
) -> Result<(Account, Option<AccountMailTransport>), AppError> {
    let email =
        EmailAddress::parse(&cmd.email).ok_or_else(|| AppError::validation("邮箱地址无效"))?;
    let now = now_ms();
    let mut tx = db.begin().await?;

    let existing = accounts::get_by_email(&mut tx, &email)
        .await
        .map_err(AppError::from)?;
    let previous_transport = if let Some(account) = &existing {
        mail_transports::get(&mut tx, &account.id).await?
    } else {
        None
    };
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
            contact_delta_link: None,
            mail_sync_triggered_at: None,
            contact_synced_at: None,
            mail_sync_status: SyncStatus::Idle,
            mail_sync_error: None,
            include_in_all: true,
            sent_archive_policy: crate::domain::entities::ArchivePolicy::NeedsConfiguration,
            created_at: now,
            updated_at: now,
        };
        accounts::upsert(&mut tx, &account)
            .await
            .map_err(AppError::from)?;
        account
    };

    let transport = AccountMailTransport { account_id: account.id.clone(), receive, send, imap_idle_supported: None, updated_at: now };
    if previous_transport.as_ref().is_some_and(|old| !old.receive.same_mailbox(&transport.receive)) {
        retire_mail_identity(&mut tx, &account.id, now, "收信身份或端点已改变，旧操作已停止").await?;
    }
    mail_transports::upsert(&mut tx, &transport)
        .await
        .map_err(AppError::from)?;
    tx.commit().await?;

    Ok((account, previous_transport))
}

pub(crate) async fn retire_mail_identity(
    conn: &mut sqlx::SqliteConnection,
    account_id: &AccountId,
    now: i64,
    reason: &str,
) -> Result<(), AppError> {
    let folders =
        crate::infrastructure::repository::folder_states::list_by_account(conn, account_id).await?;
    for folder in folders {
        crate::infrastructure::repository::folder_states::retire(
            conn,
            account_id,
            &folder.folder_id,
            now,
        )
        .await?;
        let affected = sqlx::query_as::<_, (String, String)>(
            "SELECT id, session_id FROM messages WHERE account_id = ? AND folder_id = ?",
        )
        .bind(account_id.as_str())
        .bind(folder.folder_id.as_str())
        .fetch_all(&mut *conn)
        .await?;
        for (mid, sid) in affected {
            let mid = crate::domain::value_objects::MessageId::new(mid);
            crate::infrastructure::repository::messages::set_location_status(
                conn,
                account_id,
                &mid,
                crate::domain::mail::LocationStatus::Invalidated,
            )
            .await?;
            crate::infrastructure::repository::mutations::stop_for_message(
                conn, account_id, &mid, reason,
            )
            .await?;
            crate::infrastructure::repository::sessions::recompute_projection(
                conn,
                account_id,
                &crate::domain::value_objects::SessionId::new(sid),
            )
            .await?;
        }
    }
    Ok(())
}

/// 启动恢复：仅删除没有配置引用的暂存/旧凭据；与配置切换互斥。
pub async fn recover_credentials(db: &SqlitePool, paths: &AppPaths) -> Result<(), AppError> {
    let _lock = credentials::lock(&paths.credentials_dir)?;
    for (kind, receive_protocol, send_protocol) in [("mail", "imap", "smtp"), ("graph", "graph", "graph")] {
        let references = sqlx::query_scalar::<_, String>(
            "SELECT receive_credential_ref FROM account_mail_transport WHERE receive_protocol = ? UNION SELECT send_credential_ref FROM account_mail_transport WHERE send_protocol = ?",
        ).bind(receive_protocol).bind(send_protocol).fetch_all(db).await?;
        for reference in crate::infrastructure::secret_store::list_ids(&paths.credentials_dir, kind)? {
            if !references.contains(&reference) { crate::infrastructure::secret_store::delete(&paths.credentials_dir, kind, &reference)?; }
        }
    }
    Ok(())
}
