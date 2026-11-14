//! 同步用例：按账号协议路由 Graph / IMAP，复用本地仓储与 ChangeSet。

use crate::application::dto::remote_models::{GraphMessageChange, RemoteMessage};
use crate::application::dto::{
    ChangeOrigin, ChangeSet, ContactSyncProgress, MailSyncProgress, TriggerContactSyncCommand,
    TriggerMailSyncCommand,
};
use crate::application::error::AppError;
use crate::domain::entities::{
    Contact, Conversation, ConversationMessageLink, MailFolderSyncState, Message, MessageRecipient,
    MessageWithRecipients,
};
use crate::domain::value_objects::{
    AccountId, ContactId, ConversationId, ConversationKind, FolderId, MailAuthKind, MailTlsMode,
    MessageId, MessageSyncStatus, ReceiveProtocol, SyncStatus, TimestampMs,
};
use crate::infrastructure::config::load_azure_auth_config;
use crate::infrastructure::graph;
use crate::infrastructure::imap::{
    sync_account_blocking, FolderSyncRequest, ImapAuth, ImapEndpoint,
};
use crate::infrastructure::mail::credentials;
use crate::infrastructure::mail::cursor::{encode_last_uid, encode_uidvalidity};
use crate::infrastructure::paths::AppPaths;
use crate::infrastructure::repository::{
    accounts, contacts, conversations, folder_states, locate_pending, mail_transports, messages,
    mutations,
};
use crate::application::dto::remote_models::RemoteMessageLocation;
use crate::runtime::ChangeDispatcher;
use sqlx::SqlitePool;
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

fn now_ms() -> TimestampMs {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 请求同步指定账户邮件；按 `receive_protocol` 路由。
pub async fn trigger_mail_sync(
    db: &SqlitePool,
    paths: &AppPaths,
    changes: &ChangeDispatcher,
    cmd: TriggerMailSyncCommand,
) -> Result<(), AppError> {
    let mut conn = db.acquire().await?;
    let account = accounts::get(&mut conn, &cmd.account_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("账户不存在: {}", cmd.account_id)))?;
    let transport = mail_transports::ensure_default(&mut conn, &cmd.account_id, now_ms())
        .await
        .map_err(AppError::from)?;
    drop(conn);

    {
        let mut c = db.acquire().await?;
        accounts::update_mail_sync_meta(&mut c, &cmd.account_id, SyncStatus::Running, None, None)
            .await
            .map_err(AppError::from)?;
    }

    let result = match transport.receive_protocol {
        ReceiveProtocol::Graph => sync_graph_mail(db, paths, &cmd).await,
        ReceiveProtocol::Imap => sync_imap_mail(db, paths, &cmd, &transport).await,
    };

    match result {
        Ok(changeset) => {
            let mut c = db.acquire().await?;
            accounts::update_mail_sync_meta(
                &mut c,
                &cmd.account_id,
                SyncStatus::Idle,
                None,
                Some(now_ms()),
            )
            .await
            .map_err(AppError::from)?;
            if !changeset.is_empty() {
                changes.dispatch(changeset);
            }
            let _ = account;
            Ok(())
        }
        Err(e) => {
            if let Ok(mut c) = db.acquire().await {
                let _ = accounts::update_mail_sync_meta(
                    &mut c,
                    &cmd.account_id,
                    SyncStatus::Error,
                    Some(&e.message),
                    None,
                )
                .await;
            }
            Err(e)
        }
    }
}

async fn sync_graph_mail(
    db: &SqlitePool,
    paths: &AppPaths,
    cmd: &TriggerMailSyncCommand,
) -> Result<ChangeSet, AppError> {
    let config = load_azure_auth_config(&paths.config_dir).map_err(AppError::from)?;
    let token = graph::auth::get_access_token(&paths.credentials_dir, &config, &cmd.account_id)
        .await
        .map_err(AppError::from)?;

    let folders: Vec<FolderId> = cmd
        .folder_ids
        .clone()
        .filter(|f| !f.is_empty())
        .unwrap_or_else(|| vec![FolderId::new("inbox")]);

    let mut changeset = ChangeSet::empty(cmd.account_id.clone(), ChangeOrigin::IncrementalSync);
    let mut is_initial = false;

    for folder in folders {
        let prev = {
            let mut conn = db.acquire().await?;
            folder_states::get(&mut conn, &cmd.account_id, &folder)
                .await
                .map_err(AppError::from)?
        };
        if prev.as_ref().and_then(|p| p.delta_link.as_ref()).is_none() {
            is_initial = true;
        }
        let resume = prev
            .as_ref()
            .and_then(|p| p.next_link.as_deref().or(p.delta_link.as_deref()))
            .map(|s| s.to_string());

        let page = graph::mail::delta_messages(&token, &folder, resume.as_deref())
            .await
            .map_err(AppError::from)?;

        let mut tx = db.begin().await?;
        for change in page.messages {
            match change {
                GraphMessageChange::Upsert(remote) => {
                    let is_draft = remote.is_draft;
                    let (mid, cid, is_new) =
                        upsert_remote_message(&mut tx, &cmd.account_id, &remote).await?;
                    changeset.message_ids.push(mid.clone());
                    if let Some(cid) = cid {
                        changeset.conversation_ids.push(cid);
                    }
                    if is_new && !is_draft {
                        changeset.new_incoming_ids.push(mid);
                    }
                }
                GraphMessageChange::Remove { graph_id } => {
                    // Remove 仅移出信号：入队核对，事务外 locate 后再软删/改位置
                    locate_pending::enqueue(
                        &mut tx,
                        &cmd.account_id,
                        &graph_id,
                        &folder,
                        now_ms(),
                    )
                    .await
                    .map_err(AppError::from)?;
                }
            }
        }

        let state = MailFolderSyncState {
            account_id: cmd.account_id.clone(),
            folder_id: folder,
            delta_link: page.delta_link.or_else(|| {
                prev.as_ref().and_then(|p| p.delta_link.clone())
            }),
            next_link: page.next_link,
            synced_at: Some(now_ms()),
            sync_status: SyncStatus::Idle,
            last_error: None,
            updated_at: now_ms(),
        };
        folder_states::upsert(&mut tx, &state)
            .await
            .map_err(AppError::from)?;
        tx.commit().await?;
    }

    // 事务外处理到期核对
    let locate_changes = process_due_locates(db, paths, &cmd.account_id, 32).await?;
    changeset.message_ids.extend(locate_changes.message_ids);
    changeset.conversation_ids.extend(locate_changes.conversation_ids);

    if is_initial {
        changeset.origin = ChangeOrigin::InitialSync;
    }
    let mut seen = HashSet::new();
    changeset.conversation_ids.retain(|c| seen.insert(c.clone()));
    Ok(changeset)
}

async fn sync_imap_mail(
    db: &SqlitePool,
    paths: &AppPaths,
    cmd: &TriggerMailSyncCommand,
    transport: &crate::domain::entities::AccountMailTransport,
) -> Result<ChangeSet, AppError> {
    let host = transport
        .imap_host
        .clone()
        .ok_or_else(|| AppError::validation("IMAP 账号缺少 imap_host"))?;
    let port = transport
        .imap_port
        .ok_or_else(|| AppError::validation("IMAP 账号缺少 imap_port"))?;
    let tls = transport
        .imap_tls
        .unwrap_or(MailTlsMode::Implicit);
    let username = transport
        .username
        .clone()
        .ok_or_else(|| AppError::validation("IMAP 账号缺少 username"))?;
    let secret = credentials::load(&paths.credentials_dir, &cmd.account_id)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::auth("缺少 IMAP 凭证"))?;

    let auth = match transport.auth_kind {
        MailAuthKind::Password => ImapAuth::Password {
            username: username.clone(),
            password: secret.password.ok_or_else(|| AppError::auth("缺少 IMAP 密码"))?,
        },
        MailAuthKind::OAuth2 => ImapAuth::OAuth2 {
            username: username.clone(),
            access_token: secret
                .access_token
                .ok_or_else(|| AppError::auth("缺少 IMAP OAuth access_token"))?,
        },
    };

    let endpoint = ImapEndpoint { host, port, tls };

    // 先收集各文件夹游标与已知 UID
    let mut requests = Vec::new();
    let mut is_initial = false;
    {
        let mut conn = db.acquire().await?;
        let folder_names: Vec<String> = if let Some(ids) = &cmd.folder_ids {
            ids.iter().map(|f| f.as_str().to_string()).collect()
        } else {
            vec!["INBOX".into()]
        };
        for folder in folder_names {
            let prev =
                folder_states::get(&mut conn, &cmd.account_id, &FolderId::new(folder.clone()))
                    .await
                    .map_err(AppError::from)?;
            if prev.as_ref().and_then(|p| p.delta_link.as_ref()).is_none() {
                is_initial = true;
            }
            let known = load_known_uids(&mut conn, &cmd.account_id, &folder).await?;
            requests.push(FolderSyncRequest {
                folder,
                stored_delta: prev.as_ref().and_then(|p| p.delta_link.clone()),
                stored_next: prev.as_ref().and_then(|p| p.next_link.clone()),
                known_uids: known,
            });
        }
    }

    let endpoint2 = endpoint.clone();
    let auth2 = auth.clone();
    let batch = tokio::task::spawn_blocking(move || {
        sync_account_blocking(&endpoint2, &auth2, &requests, 200)
    })
    .await
    .map_err(|e| AppError::internal(format!("IMAP 同步任务失败: {e}")))?
    .map_err(AppError::from)?;

    {
        let mut conn = db.acquire().await?;
        let mut t = transport.clone();
        t.imap_idle_supported = Some(batch.idle_supported);
        t.updated_at = now_ms();
        mail_transports::upsert(&mut conn, &t)
            .await
            .map_err(AppError::from)?;
    }

    let mut changeset = ChangeSet::empty(cmd.account_id.clone(), ChangeOrigin::IncrementalSync);

    for result in batch.folders {
        let mut tx = db.begin().await?;
        for change in result.changes {
            match change {
                GraphMessageChange::Upsert(remote) => {
                    let is_draft = remote.is_draft;
                    let (mid, cid, is_new) =
                        upsert_remote_message(&mut tx, &cmd.account_id, &remote).await?;
                    changeset.message_ids.push(mid.clone());
                    if let Some(cid) = cid {
                        changeset.conversation_ids.push(cid);
                    }
                    if is_new && !is_draft {
                        changeset.new_incoming_ids.push(mid);
                    }
                }
                GraphMessageChange::Remove { graph_id } => {
                    if let Some(existing) =
                        messages::get_by_graph_id(&mut tx, &cmd.account_id, &graph_id)
                            .await
                            .map_err(AppError::from)?
                    {
                        let mut msg = existing.message;
                        msg.sync_status = MessageSyncStatus::DeletedRemote;
                        msg.local_updated_at = now_ms();
                        messages::upsert_with_recipients(
                            &mut tx,
                            &MessageWithRecipients {
                                message: msg.clone(),
                                recipients: existing.recipients,
                            },
                        )
                        .await
                        .map_err(AppError::from)?;
                        changeset.message_ids.push(msg.id);
                    }
                }
            }
        }
        for gid in result.missing_remote_ids {
            let graph_id = crate::domain::value_objects::GraphImmutableId::new(gid);
            if let Some(existing) =
                messages::get_by_graph_id(&mut tx, &cmd.account_id, &graph_id)
                    .await
                    .map_err(AppError::from)?
            {
                let mut msg = existing.message;
                msg.sync_status = MessageSyncStatus::DeletedRemote;
                msg.local_updated_at = now_ms();
                messages::upsert_with_recipients(
                    &mut tx,
                    &MessageWithRecipients {
                        message: msg.clone(),
                        recipients: existing.recipients,
                    },
                )
                .await
                .map_err(AppError::from)?;
                changeset.message_ids.push(msg.id);
            }
        }

        let state = MailFolderSyncState {
            account_id: cmd.account_id.clone(),
            folder_id: FolderId::new(result.folder),
            delta_link: Some(encode_uidvalidity(result.uidvalidity)),
            next_link: Some(encode_last_uid(result.last_uid)),
            synced_at: Some(now_ms()),
            sync_status: SyncStatus::Idle,
            last_error: None,
            updated_at: now_ms(),
        };
        folder_states::upsert(&mut tx, &state)
            .await
            .map_err(AppError::from)?;
        tx.commit().await?;
    }

    if is_initial {
        changeset.origin = ChangeOrigin::InitialSync;
    }
    // 去重 conversation_ids
    let mut seen = HashSet::new();
    changeset.conversation_ids.retain(|c| seen.insert(c.clone()));
    Ok(changeset)
}

async fn load_known_uids(
    conn: &mut sqlx::SqliteConnection,
    account_id: &AccountId,
    folder: &str,
) -> Result<Vec<(u32, bool)>, AppError> {
    let prefix = format!("imap:{folder}:");
    let rows = sqlx::query_as::<_, (String, i64)>(
        "SELECT graph_id, is_read FROM messages
         WHERE account_id = ? AND folder_id = ? AND graph_id LIKE ? AND sync_status != 'deleted_remote'",
    )
    .bind(account_id.as_str())
    .bind(folder)
    .bind(format!("{prefix}%"))
    .fetch_all(&mut *conn)
    .await
    .map_err(AppError::from)?;
    let mut out = Vec::new();
    for (gid, is_read) in rows {
        if let Some(uid_str) = gid.strip_prefix(&prefix) {
            if let Ok(uid) = uid_str.parse::<u32>() {
                out.push((uid, is_read != 0));
            }
        }
    }
    Ok(out)
}

async fn upsert_remote_message(
    conn: &mut sqlx::SqliteConnection,
    account_id: &AccountId,
    remote: &RemoteMessage,
) -> Result<(MessageId, Option<ConversationId>, bool), AppError> {
    let existing = messages::get_by_graph_id(conn, account_id, &remote.graph_id)
        .await
        .map_err(AppError::from)?;
    let is_new = existing.is_none();
    let local_id = existing
        .as_ref()
        .map(|e| e.message.id.clone())
        .unwrap_or_else(|| MessageId::new(Uuid::new_v4().to_string()));

    let pending_read = mutations::get(conn, account_id, &local_id)
        .await
        .map_err(AppError::from)?;

    // FLAGS-only 占位：空 subject 且无 body → 仅更新已读（有 pending 时保留本地）
    if !is_new && remote.subject.is_empty() && remote.body_content.is_none() {
        let mut bundle = existing.unwrap();
        if let Some(p) = &pending_read {
            bundle.message.is_read = p.desired_is_read;
        } else {
            bundle.message.is_read = remote.is_read;
        }
        bundle.message.local_updated_at = now_ms();
        messages::upsert_with_recipients(conn, &bundle)
            .await
            .map_err(AppError::from)?;
        return Ok((bundle.message.id, None, false));
    }

    let self_email = accounts::get(conn, account_id)
        .await
        .map_err(AppError::from)?
        .map(|a| a.email);
    let is_outbound = match (&remote.from_address, &self_email) {
        (Some(from), Some(me)) => from.as_str() == me.as_str(),
        _ => false,
    };

    let is_read = pending_read
        .as_ref()
        .map(|p| p.desired_is_read)
        .unwrap_or(remote.is_read);

    let body_content = if let Some(ref ex) = existing {
        remote
            .body_content
            .clone()
            .or_else(|| ex.message.body_content.clone())
    } else {
        remote.body_content.clone()
    };

    let message = Message {
        id: local_id.clone(),
        account_id: account_id.clone(),
        graph_id: Some(remote.graph_id.clone()),
        thread_id: remote.thread_id.clone(),
        graph_change_key: remote.graph_change_key.clone(),
        internet_message_id: remote.internet_message_id.clone(),
        conversation_index: remote.conversation_index.clone(),
        subject: remote.subject.clone(),
        body_preview: remote.body_preview.clone(),
        body_content_type: remote.body_content_type,
        body_content,
        from_address: remote.from_address.clone(),
        from_name: remote.from_name.clone(),
        received_at: remote.received_at.or(Some(now_ms())),
        sent_at: remote.sent_at,
        created_at_graph: remote.created_at_graph,
        is_read,
        is_draft: remote.is_draft,
        is_outbound,
        has_attachments: remote.has_attachments,
        importance: remote.importance,
        folder_id: remote.folder_id.clone(),
        parent_folder_name: remote.parent_folder_name.clone(),
        sync_status: MessageSyncStatus::Synced,
        local_updated_at: now_ms(),
        remote_updated_at: remote.remote_updated_at,
    };
    let recipients: Vec<_> = remote
        .recipients
        .iter()
        .map(|r| MessageRecipient {
            account_id: account_id.clone(),
            message_id: local_id.clone(),
            role: r.role,
            seq: r.seq,
            normalized_address: r.normalized_address.clone(),
            display_name: r.display_name.clone(),
        })
        .collect();
    messages::upsert_with_recipients(
        conn,
        &MessageWithRecipients {
            message: message.clone(),
            recipients,
        },
    )
    .await
    .map_err(AppError::from)?;

    let peer = if is_outbound {
        remote
            .recipients
            .iter()
            .find(|r| {
                matches!(
                    r.role,
                    crate::domain::value_objects::RecipientRole::To
                        | crate::domain::value_objects::RecipientRole::Cc
                )
            })
            .map(|r| r.normalized_address.clone())
    } else {
        remote.from_address.clone()
    };

    let conversation_id = if let Some(peer) = peer {
        Some(ensure_contact_conversation_local(conn, account_id, &peer, &local_id).await?)
    } else {
        None
    };

    Ok((local_id, conversation_id, is_new))
}

async fn ensure_contact_conversation_local(
    conn: &mut sqlx::SqliteConnection,
    account_id: &AccountId,
    peer: &crate::domain::value_objects::EmailAddress,
    message_id: &MessageId,
) -> Result<ConversationId, AppError> {
    let existing = sqlx::query_scalar::<_, String>(
        "SELECT id FROM conversations WHERE account_id = ? AND kind = 'contact' AND normalized_address = ?",
    )
    .bind(account_id.as_str())
    .bind(peer.as_str())
    .fetch_optional(&mut *conn)
    .await
    .map_err(AppError::from)?;

    let cid = if let Some(id) = existing {
        ConversationId::new(id)
    } else {
        let contact = contacts::get_by_address(conn, account_id, peer)
            .await
            .map_err(AppError::from)?;
        let cid = ConversationId::new(format!("conv-{}", Uuid::new_v4()));
        let now = now_ms();
        conversations::upsert(
            conn,
            &Conversation {
                id: cid.clone(),
                account_id: account_id.clone(),
                kind: ConversationKind::Contact,
                normalized_address: Some(peer.clone()),
                contact_id: contact.map(|c| c.id),
                group_id: None,
                last_message_at: Some(now),
                last_message_id: Some(message_id.clone()),
                unread_count: 0,
                created_at: now,
                updated_at: now,
            },
        )
        .await
        .map_err(AppError::from)?;
        cid
    };

    conversations::attach_messages(
        conn,
        &[ConversationMessageLink {
            account_id: account_id.clone(),
            conversation_id: cid.clone(),
            message_id: message_id.clone(),
            attached_at: now_ms(),
        }],
    )
    .await
    .map_err(AppError::from)?;
    conversations::recompute_projection(conn, account_id, &cid)
        .await
        .map_err(AppError::from)?;
    Ok(cid)
}

pub async fn trigger_contact_sync(
    db: &SqlitePool,
    paths: &AppPaths,
    cmd: TriggerContactSyncCommand,
) -> Result<(), AppError> {
    let transport = {
        let mut conn = db.acquire().await?;
        mail_transports::ensure_default(&mut conn, &cmd.account_id, now_ms())
            .await
            .map_err(AppError::from)?
    };
    if transport.receive_protocol != ReceiveProtocol::Graph {
        // IMAP 无 Graph 通讯录 Delta；直接成功返回
        return Ok(());
    }

    let config = load_azure_auth_config(&paths.config_dir).map_err(AppError::from)?;
    let token = graph::auth::get_access_token(&paths.credentials_dir, &config, &cmd.account_id)
        .await
        .map_err(AppError::from)?;

    let mut link = {
        let mut conn = db.acquire().await?;
        accounts::get(&mut conn, &cmd.account_id)
            .await
            .map_err(AppError::from)?
            .and_then(|a| a.contact_delta_link)
    };

    // 最多拉若干页，避免一次阻塞过久
    for _ in 0..20 {
        let page = graph::contacts::delta_contacts(&token, link.as_deref())
            .await
            .map_err(AppError::from)?;
        let mut tx = db.begin().await?;
        for remote in page.upserts {
            let existing = contacts::get_by_graph_id(&mut tx, &cmd.account_id, &remote.graph_id)
                .await
                .map_err(AppError::from)?;
            let by_addr =
                contacts::get_by_address(&mut tx, &cmd.account_id, &remote.normalized_address)
                    .await
                    .map_err(AppError::from)?;
            let now = now_ms();
            let contact = if let Some(mut c) = existing.or(by_addr) {
                c.graph_id = Some(remote.graph_id.clone());
                c.display_name = remote.display_name;
                c.normalized_address = remote.normalized_address;
                c.updated_at = now;
                c
            } else {
                Contact {
                    id: ContactId::new(Uuid::new_v4().to_string()),
                    account_id: cmd.account_id.clone(),
                    graph_id: Some(remote.graph_id),
                    display_name: remote.display_name,
                    normalized_address: remote.normalized_address,
                    created_at: now,
                    updated_at: now,
                }
            };
            contacts::upsert(&mut tx, &contact)
                .await
                .map_err(AppError::from)?;
        }
        for gid in page.removed_graph_ids {
            if let Some(c) = contacts::get_by_graph_id(&mut tx, &cmd.account_id, &gid)
                .await
                .map_err(AppError::from)?
            {
                contacts::delete(&mut tx, &cmd.account_id, &c.id)
                    .await
                    .map_err(AppError::from)?;
            }
        }
        let next = page.next_link.clone();
        let delta = page.delta_link.clone();
        if let Some(ref d) = delta {
            accounts::update_contact_sync_meta(&mut tx, &cmd.account_id, Some(d), Some(now_ms()))
                .await
                .map_err(AppError::from)?;
        }
        tx.commit().await?;

        if let Some(n) = next {
            link = Some(n);
            continue;
        }
        if let Some(d) = delta {
            let mut conn = db.acquire().await?;
            accounts::update_contact_sync_meta(&mut conn, &cmd.account_id, Some(&d), Some(now_ms()))
                .await
                .map_err(AppError::from)?;
        }
        break;
    }
    Ok(())
}

/// 处理到期的 Remove 核对记录。
pub async fn process_due_locates(
    db: &SqlitePool,
    paths: &AppPaths,
    account_id: &AccountId,
    limit: u32,
) -> Result<ChangeSet, AppError> {
    let due = {
        let mut conn = db.acquire().await?;
        locate_pending::list_due(&mut conn, account_id, now_ms(), limit)
            .await
            .map_err(AppError::from)?
    };
    let mut changeset = ChangeSet::empty(account_id.clone(), ChangeOrigin::IncrementalSync);
    if due.is_empty() {
        return Ok(changeset);
    }

    let config = load_azure_auth_config(&paths.config_dir).map_err(AppError::from)?;
    let token = graph::auth::get_access_token(&paths.credentials_dir, &config, account_id)
        .await
        .map_err(AppError::from)?;

    for item in due {
        match graph::mail::locate_message(&token, &item.graph_id).await {
            Ok(RemoteMessageLocation::NotFound) => {
                let mut tx = db.begin().await?;
                if !locate_pending::complete_if_revision(
                    &mut tx,
                    account_id,
                    &item.graph_id,
                    item.revision,
                )
                .await
                .map_err(AppError::from)?
                {
                    tx.rollback().await.ok();
                    continue;
                }
                if let Some(existing) =
                    messages::get_by_graph_id(&mut tx, account_id, &item.graph_id)
                        .await
                        .map_err(AppError::from)?
                {
                    let mut msg = existing.message;
                    msg.sync_status = MessageSyncStatus::DeletedRemote;
                    msg.local_updated_at = now_ms();
                    messages::upsert_with_recipients(
                        &mut tx,
                        &MessageWithRecipients {
                            message: msg.clone(),
                            recipients: existing.recipients,
                        },
                    )
                    .await
                    .map_err(AppError::from)?;
                    changeset.message_ids.push(msg.id);
                }
                tx.commit().await?;
            }
            Ok(RemoteMessageLocation::Present { folder_id }) => {
                let mut tx = db.begin().await?;
                if !locate_pending::complete_if_revision(
                    &mut tx,
                    account_id,
                    &item.graph_id,
                    item.revision,
                )
                .await
                .map_err(AppError::from)?
                {
                    tx.rollback().await.ok();
                    continue;
                }
                if let Some(existing) =
                    messages::get_by_graph_id(&mut tx, account_id, &item.graph_id)
                        .await
                        .map_err(AppError::from)?
                {
                    let mut msg = existing.message;
                    msg.folder_id = Some(folder_id);
                    msg.sync_status = MessageSyncStatus::Synced;
                    msg.local_updated_at = now_ms();
                    messages::upsert_with_recipients(
                        &mut tx,
                        &MessageWithRecipients {
                            message: msg.clone(),
                            recipients: existing.recipients,
                        },
                    )
                    .await
                    .map_err(AppError::from)?;
                    changeset.message_ids.push(msg.id);
                }
                tx.commit().await?;
            }
            Err(e) => {
                if !e_retryable(&e) {
                    // 非可恢复：仍记失败退避，不当删除
                }
                let mut conn = db.acquire().await?;
                let available_at = now_ms() + 30_000;
                locate_pending::record_failure(
                    &mut conn,
                    account_id,
                    &item.graph_id,
                    item.revision,
                    &e.to_string(),
                    available_at,
                    now_ms(),
                )
                .await
                .map_err(AppError::from)?;
            }
        }
    }
    Ok(changeset)
}

fn e_retryable(e: &crate::application::error::PortError) -> bool {
    match e {
        crate::application::error::PortError::Remote { retryable, .. } => *retryable,
        crate::application::error::PortError::Network { .. } => true,
        crate::application::error::PortError::Auth { .. } => false,
        _ => false,
    }
}

pub async fn get_mail_sync_progress(
    db: &SqlitePool,
    account_id: &AccountId,
) -> Result<MailSyncProgress, AppError> {
    let mut conn = db.acquire().await?;
    let account = accounts::get(&mut conn, account_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("账户不存在: {account_id}")))?;
    let folders = folder_states::list_by_account(&mut conn, account_id).await?;
    Ok(MailSyncProgress {
        account_id: account_id.clone(),
        mail_sync_status: account.mail_sync_status,
        mail_sync_error: account.mail_sync_error,
        mail_synced_at: account.mail_synced_at,
        folders,
    })
}

pub async fn get_contact_sync_progress(
    db: &SqlitePool,
    account_id: &AccountId,
) -> Result<ContactSyncProgress, AppError> {
    let mut conn = db.acquire().await?;
    let account = accounts::get(&mut conn, account_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("账户不存在: {account_id}")))?;
    Ok(ContactSyncProgress {
        account_id: account_id.clone(),
        contact_synced_at: account.contact_synced_at,
        contact_delta_link: account.contact_delta_link,
    })
}

