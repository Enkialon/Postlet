//! 外发：将 PendingLocal 按 send_protocol 推到 Graph / SMTP。

use crate::application::error::AppError;
use crate::domain::entities::MessageWithRecipients;
use crate::domain::value_objects::{
    AccountId, BodyContentType, MailAuthKind, MailTlsMode, MessageSyncStatus, RecipientRole,
    SendProtocol,
};
use crate::infrastructure::config::load_azure_auth_config;
use crate::infrastructure::graph::{self, mail::GraphOutgoing};
use crate::infrastructure::mail::credentials;
use crate::infrastructure::paths::AppPaths;
use crate::infrastructure::repository::{accounts, mail_transports, messages};
use crate::infrastructure::smtp::{self, SmtpAuth, SmtpEndpoint, SmtpOutgoing};
use sqlx::SqlitePool;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 发送指定本地邮件（须为 PendingLocal）；成功后标为 Synced。
pub async fn flush_pending_message(
    db: &SqlitePool,
    paths: &AppPaths,
    account_id: &AccountId,
    message_id: &crate::domain::value_objects::MessageId,
) -> Result<(), AppError> {
    let mut conn = db.acquire().await?;
    let bundle = messages::get(&mut conn, account_id, message_id)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::not_found(format!("邮件不存在: {message_id}")))?;
    if bundle.message.sync_status != MessageSyncStatus::PendingLocal {
        return Ok(());
    }
    if bundle.message.has_attachments {
        return Err(AppError::validation("附件内容不可用，已阻止不完整发送"));
    }
    let account = accounts::get(&mut conn, account_id)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::not_found(format!("账户不存在: {account_id}")))?;
    let transport = mail_transports::require(&mut conn, account_id)
        .await
        .map_err(AppError::from)?;
    drop(conn);

    match transport.send_protocol {
        SendProtocol::Graph => {
            send_via_graph(paths, &transport, &bundle).await?;
        }
        SendProtocol::Smtp => {
            send_via_smtp(paths, &account, &transport, &bundle).await?;
        }
    }

    let mut conn = db.acquire().await?;
    let mut updated = bundle;
    updated.message.sync_status = MessageSyncStatus::Synced;
    updated.message.local_updated_at = now_ms();
    messages::upsert_with_recipients(&mut conn, &updated)
        .await
        .map_err(AppError::from)?;
    Ok(())
}

async fn send_via_graph(
    paths: &AppPaths,
    transport: &crate::domain::entities::AccountMailTransport,
    bundle: &MessageWithRecipients,
) -> Result<(), AppError> {
    let config = load_azure_auth_config(&paths.config_dir).map_err(AppError::from)?;
    let token = graph::auth::get_access_token(&paths.credentials_dir, &config, graph::auth::credential_reference(transport)?)
        .await
        .map_err(AppError::from)?;

    let mut to = Vec::new();
    let mut cc = Vec::new();
    let mut bcc = Vec::new();
    for r in &bundle.recipients {
        let pair = (
            r.normalized_address.as_str().to_string(),
            r.display_name.clone(),
        );
        match r.role {
            RecipientRole::To => to.push(pair),
            RecipientRole::Cc => cc.push(pair),
            RecipientRole::Bcc => bcc.push(pair),
            RecipientRole::ReplyTo => {}
        }
    }

    let body_content = bundle
        .message
        .body_content
        .clone()
        .unwrap_or_else(|| bundle.message.body_preview.clone());

    graph::mail::send_mail(
        &token,
        &GraphOutgoing {
            subject: bundle.message.subject.clone(),
            body_content_type: bundle.message.body_content_type,
            body_content,
            to,
            cc,
            bcc,
            save_to_sent_items: true,
        },
    )
    .await
    .map_err(AppError::from)
}

async fn send_via_smtp(
    paths: &AppPaths,
    account: &crate::domain::entities::Account,
    transport: &crate::domain::entities::AccountMailTransport,
    bundle: &MessageWithRecipients,
) -> Result<(), AppError> {
    let host = transport
        .smtp_host
        .clone()
        .ok_or_else(|| AppError::validation("SMTP 账号缺少 smtp_host"))?;
    let port = transport
        .smtp_port
        .ok_or_else(|| AppError::validation("SMTP 账号缺少 smtp_port"))?;
    let tls = transport.smtp_tls.unwrap_or(MailTlsMode::StartTls);
    let username = transport
        .username
        .clone()
        .unwrap_or_else(|| account.email.as_str().to_string());
    let secret = credentials::load(&paths.credentials_dir, &transport)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::auth("缺少 SMTP 凭证"))?;

    let auth = match transport.auth_kind {
        MailAuthKind::Password => SmtpAuth::Password {
            username,
            password: secret
                .password
                .ok_or_else(|| AppError::auth("缺少 SMTP 密码"))?,
        },
        MailAuthKind::OAuth2 => SmtpAuth::OAuth2 {
            username,
            access_token: secret
                .access_token
                .ok_or_else(|| AppError::auth("缺少 SMTP OAuth access_token"))?,
        },
    };

    let mut to = Vec::new();
    let mut cc = Vec::new();
    let mut bcc = Vec::new();
    for r in &bundle.recipients {
        match r.role {
            RecipientRole::To => to.push(r.normalized_address.as_str().to_string()),
            RecipientRole::Cc => cc.push(r.normalized_address.as_str().to_string()),
            RecipientRole::Bcc => bcc.push(r.normalized_address.as_str().to_string()),
            RecipientRole::ReplyTo => {}
        }
    }

    let (text_body, html_body) = match bundle.message.body_content_type {
        BodyContentType::Html => (
            Some(
                bundle
                    .message
                    .body_preview
                    .clone(),
            ),
            bundle.message.body_content.clone(),
        ),
        BodyContentType::Text => (bundle.message.body_content.clone(), None),
    };

    let in_reply_to = bundle.message.internet_message_id.clone();
    let outgoing = SmtpOutgoing {
        from: account.email.as_str().to_string(),
        from_name: account.display_name.clone(),
        to,
        cc,
        bcc,
        subject: bundle.message.subject.clone(),
        text_body,
        html_body,
        in_reply_to,
        references: bundle.message.internet_message_id.clone(),
        attachments: Vec::new(),
    };

    smtp::send_message(
        &SmtpEndpoint { host, port, tls },
        &auth,
        &outgoing,
    )
    .await
    .map_err(AppError::from)?;
    Ok(())
}
