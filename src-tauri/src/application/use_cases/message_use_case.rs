//! 邮件用例：本地读写、正文按需、已读/删除推远端。

use crate::application::dto::{
    ChangeOrigin, ChangeSet, DeleteMessagesCommand, MarkMessagesReadCommand, PendingMutationKind,
};
use crate::application::error::{AppError, PortError};
use crate::domain::entities::MessageWithRecipients;
use crate::domain::value_objects::{
    AccountId, MailAuthKind, MailTlsMode, MessageId, ReceiveProtocol, TimestampMs,
};
use crate::infrastructure::config::load_azure_auth_config;
use crate::infrastructure::graph;
use crate::infrastructure::imap::{
    uid_delete_async, uid_set_seen_async, ImapAuth, ImapEndpoint,
};
use crate::infrastructure::mail::credentials;
use crate::infrastructure::paths::AppPaths;
use crate::infrastructure::repository::{
    accounts, mail_transports, messages, mutations, sessions,
};
use sqlx::SqlitePool;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ms() -> TimestampMs {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 本地入队后异步冲刷远端；不阻塞用例返回（调度器仍会兜底）。
fn spawn_flush_due_mutations(db: &SqlitePool, paths: &AppPaths, account_id: &AccountId) {
    let db = db.clone();
    let paths = paths.clone();
    let account_id = account_id.clone();
    tauri::async_runtime::spawn(async move {
        let _ = flush_due_mutations(&db, &paths, &account_id, 32).await;
    });
}

fn backoff_ms(attempts: i32) -> i64 {
    let exp = attempts.clamp(0, 6) as u32;
    (5_000i64).saturating_mul(1i64 << exp).min(15 * 60_000)
}

/// 在指定账户内读取本地邮件及收件人；不存在时返回未找到错误。
pub async fn get_message(
    db: &SqlitePool,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<MessageWithRecipients, AppError> {
    let mut conn = db.acquire().await?;
    messages::get(&mut conn, account_id, message_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("邮件不存在: {message_id}")))
}

/// 确保邮件正文已加载；本地缺失时按协议补取。
pub async fn ensure_message_body(
    db: &SqlitePool,
    paths: &AppPaths,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<MessageWithRecipients, AppError> {
    let mut bundle = get_message(db, account_id, message_id).await?;
    if bundle.message.body_content.is_some() {
        return Ok(bundle);
    }

    let transport = {
        let mut conn = db.acquire().await?;
        mail_transports::ensure_default(&mut conn, account_id, now_ms())
            .await
            .map_err(AppError::from)?
    };

    match transport.receive_protocol {
        ReceiveProtocol::Imap => {
            // IMAP 同步路径通常已带正文；缺失时不再二次 FETCH（避免重复协议细节）
            Ok(bundle)
        }
        ReceiveProtocol::Graph => {
            let Some(graph_id) = bundle.message.graph_id.clone() else {
                return Ok(bundle);
            };
            let config = load_azure_auth_config(&paths.config_dir).map_err(AppError::from)?;
            let token = graph::auth::get_access_token(&paths.credentials_dir, &config, account_id)
                .await
                .map_err(AppError::from)?;
            let (content_type, content) = graph::mail::get_message_body(&token, &graph_id)
                .await
                .map_err(AppError::from)?;
            bundle.message.body_content_type = content_type;
            bundle.message.body_content = Some(content);
            bundle.message.local_updated_at = now_ms();
            let mut conn = db.acquire().await?;
            messages::upsert_with_recipients(&mut conn, &bundle)
                .await
                .map_err(AppError::from)?;
            Ok(bundle)
        }
    }
}

/// 批量修改已读：本地更新 + 入队后立即返回；远端由后台 flush。
pub async fn mark_read(
    db: &SqlitePool,
    paths: &AppPaths,
    cmd: MarkMessagesReadCommand,
) -> Result<ChangeSet, AppError> {
    let mut tx = db.begin().await?;
    let changed =
        messages::mark_read(&mut tx, &cmd.account_id, &cmd.message_ids, cmd.is_read).await?;
    for mid in &changed {
        mutations::put_read(&mut tx, &cmd.account_id, mid, cmd.is_read)
            .await
            .map_err(AppError::from)?;
    }
    let session_ids = messages::list_session_ids(&mut tx, &cmd.account_id, &changed).await?;
    for sid in &session_ids {
        sessions::recompute_projection(&mut tx, &cmd.account_id, sid).await?;
    }
    tx.commit().await?;

    if !changed.is_empty() {
        spawn_flush_due_mutations(db, paths, &cmd.account_id);
    }

    Ok(ChangeSet {
        account_id: cmd.account_id,
        session_ids,
        message_ids: changed,
        new_incoming_ids: Vec::new(),
        origin: ChangeOrigin::UserAction,
    })
}

/// 批量删除邮件：本地软删 + 入队后立即返回；远端由后台 flush。
pub async fn delete_messages(
    db: &SqlitePool,
    paths: &AppPaths,
    cmd: DeleteMessagesCommand,
) -> Result<ChangeSet, AppError> {
    if cmd.message_ids.is_empty() {
        return Ok(ChangeSet {
            account_id: cmd.account_id,
            session_ids: Vec::new(),
            message_ids: Vec::new(),
            new_incoming_ids: Vec::new(),
            origin: ChangeOrigin::UserAction,
        });
    }

    let mut tx = db.begin().await?;
    let mut changed = Vec::new();
    for mid in &cmd.message_ids {
        let Some(bundle) = messages::get(&mut tx, &cmd.account_id, mid)
            .await
            .map_err(AppError::from)?
        else {
            continue;
        };
        if bundle.message.sync_status
            == crate::domain::value_objects::MessageSyncStatus::DeletedRemote
        {
            continue;
        }
        messages::mark_deleted_remote(&mut tx, &cmd.account_id, &[mid.clone()])
            .await
            .map_err(AppError::from)?;
        mutations::put_delete(&mut tx, &cmd.account_id, mid)
            .await
            .map_err(AppError::from)?;
        changed.push(mid.clone());
    }

    let session_ids = messages::list_session_ids(&mut tx, &cmd.account_id, &changed).await?;
    for sid in &session_ids {
        sessions::recompute_projection(&mut tx, &cmd.account_id, sid).await?;
    }
    tx.commit().await?;

    if !changed.is_empty() {
        spawn_flush_due_mutations(db, paths, &cmd.account_id);
    }

    Ok(ChangeSet {
        account_id: cmd.account_id,
        session_ids,
        message_ids: changed,
        new_incoming_ids: Vec::new(),
        origin: ChangeOrigin::UserAction,
    })
}

/// 串行推送到期的已读 / 删除变更。
pub async fn flush_due_mutations(
    db: &SqlitePool,
    paths: &AppPaths,
    account_id: &AccountId,
    limit: u32,
) -> Result<u32, AppError> {
    let due = {
        let mut conn = db.acquire().await?;
        mutations::list_due(&mut conn, account_id, now_ms(), limit)
            .await
            .map_err(AppError::from)?
    };
    if due.is_empty() {
        return Ok(0);
    }

    let transport = {
        let mut conn = db.acquire().await?;
        mail_transports::ensure_default(&mut conn, account_id, now_ms())
            .await
            .map_err(AppError::from)?
    };

    let mut flushed = 0u32;
    for mutation in due {
        let push = match (mutation.kind, transport.receive_protocol) {
            (PendingMutationKind::Read, ReceiveProtocol::Graph) => {
                let is_read = mutation.desired_is_read.unwrap_or(true);
                push_graph_read(db, paths, account_id, &mutation.message_id, is_read).await
            }
            (PendingMutationKind::Read, ReceiveProtocol::Imap) => {
                let is_read = mutation.desired_is_read.unwrap_or(true);
                push_imap_read(db, paths, account_id, &mutation.message_id, is_read).await
            }
            (PendingMutationKind::Delete, ReceiveProtocol::Graph) => {
                push_graph_delete(db, paths, account_id, &mutation.message_id).await
            }
            (PendingMutationKind::Delete, ReceiveProtocol::Imap) => {
                push_imap_delete(db, paths, account_id, &mutation.message_id).await
            }
        };

        let mut conn = db.acquire().await?;
        match push {
            Ok(()) => match mutations::acknowledge(&mut conn, &mutation).await {
                Ok(()) => flushed += 1,
                Err(PortError::Conflict { .. }) => {}
                Err(e) => return Err(e.into()),
            },
            Err(e) => {
                let available_at = now_ms() + backoff_ms(mutation.attempts);
                mutations::record_failure(&mut conn, &mutation, &e.message, available_at)
                    .await
                    .map_err(AppError::from)?;
            }
        }
    }
    Ok(flushed)
}

async fn push_graph_read(
    db: &SqlitePool,
    paths: &AppPaths,
    account_id: &AccountId,
    message_id: &MessageId,
    is_read: bool,
) -> Result<(), AppError> {
    let graph_id = {
        let mut conn = db.acquire().await?;
        let bundle = messages::get(&mut conn, account_id, message_id)
            .await
            .map_err(AppError::from)?
            .ok_or_else(|| AppError::not_found(format!("邮件不存在: {message_id}")))?;
        bundle
            .message
            .graph_id
            .ok_or_else(|| AppError::validation("邮件缺少 graph_id，无法推已读"))?
    };
    let config = load_azure_auth_config(&paths.config_dir).map_err(AppError::from)?;
    let token = graph::auth::get_access_token(&paths.credentials_dir, &config, account_id)
        .await
        .map_err(AppError::from)?;
    graph::mail::patch_message_read(&token, &graph_id, is_read)
        .await
        .map_err(AppError::from)
}

async fn push_imap_read(
    db: &SqlitePool,
    paths: &AppPaths,
    account_id: &AccountId,
    message_id: &MessageId,
    is_read: bool,
) -> Result<(), AppError> {
    let (folder, uid, transport) = {
        let mut conn = db.acquire().await?;
        let bundle = messages::get(&mut conn, account_id, message_id)
            .await
            .map_err(AppError::from)?
            .ok_or_else(|| AppError::not_found(format!("邮件不存在: {message_id}")))?;
        let graph_id = bundle
            .message
            .graph_id
            .as_ref()
            .map(|g| g.as_str().to_string())
            .ok_or_else(|| AppError::validation("IMAP 邮件缺少 uid 标识"))?;
        let folder = bundle
            .message
            .folder_id
            .as_ref()
            .map(|f| f.as_str().to_string())
            .ok_or_else(|| AppError::validation("IMAP 邮件缺少 folder_id"))?;
        let prefix = format!("imap:{folder}:");
        let uid_str = graph_id
            .strip_prefix(&prefix)
            .ok_or_else(|| AppError::validation("IMAP graph_id 格式无效"))?;
        let uid: u32 = uid_str
            .parse()
            .map_err(|_| AppError::validation("IMAP uid 无效"))?;
        let transport = mail_transports::ensure_default(&mut conn, account_id, now_ms())
            .await
            .map_err(AppError::from)?;
        (folder, uid, transport)
    };

    let account = {
        let mut conn = db.acquire().await?;
        accounts::get(&mut conn, account_id)
            .await
            .map_err(AppError::from)?
            .ok_or_else(|| AppError::not_found(format!("账户不存在: {account_id}")))?
    };
    let host = transport
        .imap_host
        .clone()
        .ok_or_else(|| AppError::validation("IMAP 账号缺少 imap_host"))?;
    let port = transport
        .imap_port
        .ok_or_else(|| AppError::validation("IMAP 账号缺少 imap_port"))?;
    let tls = transport.imap_tls.unwrap_or(MailTlsMode::Implicit);
    let username = transport
        .username
        .clone()
        .unwrap_or_else(|| account.email.as_str().to_string());
    let secret = credentials::load(&paths.credentials_dir, account_id)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::auth("缺少 IMAP 凭证"))?;
    let auth = match transport.auth_kind {
        MailAuthKind::Password => ImapAuth::Password {
            username,
            password: secret
                .password
                .ok_or_else(|| AppError::auth("缺少 IMAP 密码"))?,
        },
        MailAuthKind::OAuth2 => ImapAuth::OAuth2 {
            username,
            access_token: secret
                .access_token
                .ok_or_else(|| AppError::auth("缺少 IMAP OAuth access_token"))?,
        },
    };

    uid_set_seen_async(
        &ImapEndpoint { host, port, tls },
        &auth,
        &folder,
        uid,
        is_read,
    )
    .await
    .map_err(AppError::from)
}

async fn push_graph_delete(
    db: &SqlitePool,
    paths: &AppPaths,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<(), AppError> {
    let graph_id = {
        let mut conn = db.acquire().await?;
        let bundle = messages::get(&mut conn, account_id, message_id)
            .await
            .map_err(AppError::from)?
            .ok_or_else(|| AppError::not_found(format!("邮件不存在: {message_id}")))?;
        bundle
            .message
            .graph_id
            .ok_or_else(|| AppError::validation("邮件缺少 graph_id，无法删除"))?
    };
    let config = load_azure_auth_config(&paths.config_dir).map_err(AppError::from)?;
    let token = graph::auth::get_access_token(&paths.credentials_dir, &config, account_id)
        .await
        .map_err(AppError::from)?;
    graph::mail::delete_message(&token, &graph_id)
        .await
        .map_err(AppError::from)
}

async fn push_imap_delete(
    db: &SqlitePool,
    paths: &AppPaths,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<(), AppError> {
    let (folder, uid, transport) = {
        let mut conn = db.acquire().await?;
        let bundle = messages::get(&mut conn, account_id, message_id)
            .await
            .map_err(AppError::from)?
            .ok_or_else(|| AppError::not_found(format!("邮件不存在: {message_id}")))?;
        let graph_id = bundle
            .message
            .graph_id
            .as_ref()
            .map(|g| g.as_str().to_string())
            .ok_or_else(|| AppError::validation("IMAP 邮件缺少 uid 标识"))?;
        let folder = bundle
            .message
            .folder_id
            .as_ref()
            .map(|f| f.as_str().to_string())
            .ok_or_else(|| AppError::validation("IMAP 邮件缺少 folder_id"))?;
        let prefix = format!("imap:{folder}:");
        let uid_str = graph_id
            .strip_prefix(&prefix)
            .ok_or_else(|| AppError::validation("IMAP graph_id 格式无效"))?;
        let uid: u32 = uid_str
            .parse()
            .map_err(|_| AppError::validation("IMAP uid 无效"))?;
        let transport = mail_transports::ensure_default(&mut conn, account_id, now_ms())
            .await
            .map_err(AppError::from)?;
        (folder, uid, transport)
    };

    let account = {
        let mut conn = db.acquire().await?;
        accounts::get(&mut conn, account_id)
            .await
            .map_err(AppError::from)?
            .ok_or_else(|| AppError::not_found(format!("账户不存在: {account_id}")))?
    };
    let host = transport
        .imap_host
        .clone()
        .ok_or_else(|| AppError::validation("IMAP 账号缺少 imap_host"))?;
    let port = transport
        .imap_port
        .ok_or_else(|| AppError::validation("IMAP 账号缺少 imap_port"))?;
    let tls = transport.imap_tls.unwrap_or(MailTlsMode::Implicit);
    let username = transport
        .username
        .clone()
        .unwrap_or_else(|| account.email.as_str().to_string());
    let secret = credentials::load(&paths.credentials_dir, account_id)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::auth("缺少 IMAP 凭证"))?;
    let auth = match transport.auth_kind {
        MailAuthKind::Password => ImapAuth::Password {
            username,
            password: secret
                .password
                .ok_or_else(|| AppError::auth("缺少 IMAP 密码"))?,
        },
        MailAuthKind::OAuth2 => ImapAuth::OAuth2 {
            username,
            access_token: secret
                .access_token
                .ok_or_else(|| AppError::auth("缺少 IMAP OAuth access_token"))?,
        },
    };

    uid_delete_async(&ImapEndpoint { host, port, tls }, &auth, &folder, uid)
        .await
        .map_err(AppError::from)
}
