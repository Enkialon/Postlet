//! 同步用例：按账号协议路由 Graph / IMAP，复用本地仓储与 ChangeSet。

use crate::application::dto::remote_models::{GraphMessageChange, RemoteMessage};
use crate::application::dto::{
    ChangeOrigin, ChangeSet, ContactSyncProgress, MailSyncProgress, SetFolderSyncEnabledCommand,
    TriggerContactSyncCommand, TriggerMailSyncCommand,
};
use crate::application::error::AppError;
use crate::domain::entities::{
    Contact, MailFolderSyncState, Message, MessageRecipient, MessageWithRecipients,
};
use crate::domain::value_objects::{
    AccountId, ContactId, ConversationId, FolderId, MailAuthKind, MailTlsMode, MessageId,
    MessageSyncStatus, ReceiveProtocol, SyncStatus, TimestampMs,
};
use crate::infrastructure::config::load_azure_auth_config;
use crate::infrastructure::graph;
use crate::infrastructure::imap::{
    discover_folder_tree, session_login_blocking, sync_account_blocking, FolderSyncRequest,
    ImapAuth, ImapEndpoint,
};
use crate::infrastructure::mail::credentials;
use crate::infrastructure::mail::cursor::{encode_last_uid, encode_uidvalidity};
use crate::infrastructure::paths::AppPaths;
use crate::infrastructure::repository::{
    accounts, contacts, conversations, folder_states, jobs, locate_pending, mail_transports,
    messages, mutations,
};
use crate::application::use_cases::aggregate_worker_use_case;
use crate::application::dto::remote_models::RemoteMessageLocation;
use crate::runtime::{ChangeDispatcher, MailSyncGate};
use sqlx::SqlitePool;
use std::collections::HashSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const FOLDER_REFRESH_TIMEOUT: Duration = Duration::from_secs(90);
const MAIL_SYNC_TIMEOUT: Duration = Duration::from_secs(10 * 60);

struct MailSyncGateGuard<'a> {
    gate: &'a MailSyncGate,
    account_id: String,
}

impl<'a> MailSyncGateGuard<'a> {
    fn begin(gate: &'a MailSyncGate, account_id: &str) -> Option<Self> {
        if gate.try_begin(account_id) {
            Some(Self {
                gate,
                account_id: account_id.to_string(),
            })
        } else {
            None
        }
    }
}

impl Drop for MailSyncGateGuard<'_> {
    fn drop(&mut self) {
        self.gate.end(&self.account_id);
    }
}

fn now_ms() -> TimestampMs {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// `folder_ids` 非空时按指定夹；否则同步 `sync_enabled` 夹；尚无勾选时回落到 default_folder。
async fn resolve_mail_folders(
    db: &SqlitePool,
    account_id: &AccountId,
    folder_ids: &Option<Vec<FolderId>>,
    default_folder: &str,
) -> Result<Vec<FolderId>, AppError> {
    if let Some(ids) = folder_ids {
        if !ids.is_empty() {
            return Ok(ids.clone());
        }
    }
    let mut conn = db.acquire().await?;
    let enabled = folder_states::list_sync_enabled(&mut conn, account_id)
        .await
        .map_err(AppError::from)?;
    if !enabled.is_empty() {
        return Ok(enabled.into_iter().map(|s| s.folder_id).collect());
    }
    Ok(vec![FolderId::new(default_folder)])
}

fn merge_folder_progress(
    account_id: &AccountId,
    folder: FolderId,
    prev: Option<&MailFolderSyncState>,
    delta_link: Option<String>,
    next_link: Option<String>,
) -> MailFolderSyncState {
    let now = now_ms();
    MailFolderSyncState {
        account_id: account_id.clone(),
        folder_id: folder.clone(),
        parent_folder_id: prev.and_then(|p| p.parent_folder_id.clone()),
        display_name: prev
            .map(|p| p.display_name.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| folder.as_str().to_string()),
        sync_enabled: prev.map(|p| p.sync_enabled).unwrap_or(true),
        delta_link,
        next_link,
        synced_at: Some(now),
        sync_status: SyncStatus::Idle,
        last_error: None,
        updated_at: now,
    }
}

/// 请求同步指定账户邮件；按 `receive_protocol` 路由。
///
/// 同一账户重入时直接 Ok（已在跑）；调用方可将本函数丢到后台，避免 IPC 卡死。
pub async fn trigger_mail_sync(
    db: &SqlitePool,
    paths: &AppPaths,
    changes: &ChangeDispatcher,
    gate: &MailSyncGate,
    cmd: TriggerMailSyncCommand,
) -> Result<(), AppError> {
    let Some(_guard) = MailSyncGateGuard::begin(gate, cmd.account_id.as_str()) else {
        return Ok(());
    };
    match tokio::time::timeout(
        MAIL_SYNC_TIMEOUT,
        trigger_mail_sync_locked(db, paths, changes, &cmd),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => {
            if let Ok(mut c) = db.acquire().await {
                let _ = accounts::update_mail_sync_meta(
                    &mut c,
                    &cmd.account_id,
                    SyncStatus::Error,
                    Some("邮件同步超时，请稍后重试"),
                    None,
                )
                .await;
            }
            Err(AppError::from(crate::application::error::PortError::Network {
                message: "邮件同步超时，请稍后重试".into(),
            }))
        }
    }
}

async fn trigger_mail_sync_locked(
    db: &SqlitePool,
    paths: &AppPaths,
    changes: &ChangeDispatcher,
    cmd: &TriggerMailSyncCommand,
) -> Result<(), AppError> {
    let mut conn = db.acquire().await?;
    let _account = accounts::get(&mut conn, &cmd.account_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("账户不存在: {}", cmd.account_id)))?;
    let transport = mail_transports::ensure_default(&mut conn, &cmd.account_id, now_ms())
        .await
        .map_err(AppError::from)?;
    drop(conn);

    // 账户级只记「发起」时刻；各文件夹成功时间写在 mail_folder_sync_state.synced_at
    {
        let mut c = db.acquire().await?;
        accounts::update_mail_sync_meta(
            &mut c,
            &cmd.account_id,
            SyncStatus::Running,
            None,
            Some(now_ms()),
        )
        .await
        .map_err(AppError::from)?;
    }

    let result = match transport.receive_protocol {
        ReceiveProtocol::Graph => sync_graph_mail(db, paths, cmd).await,
        ReceiveProtocol::Imap => sync_imap_mail(db, paths, cmd, &transport).await,
    };

    match result {
        Ok(changeset) => {
            let mut c = db.acquire().await?;
            accounts::update_mail_sync_meta(
                &mut c,
                &cmd.account_id,
                SyncStatus::Idle,
                None,
                None,
            )
            .await
            .map_err(AppError::from)?;
            if !changeset.is_empty() {
                changes.dispatch(changeset);
            }
            // 同步后立刻排空一批聚合，让会话列表尽快收敛
            let _ =
                aggregate_worker_use_case::process_available_jobs(db, Some(changes), 64).await;
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

    let folders =
        resolve_mail_folders(db, &cmd.account_id, &cmd.folder_ids, "inbox").await?;

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

        let state = merge_folder_progress(
            &cmd.account_id,
            folder,
            prev.as_ref(),
            page.delta_link
                .or_else(|| prev.as_ref().and_then(|p| p.delta_link.clone())),
            page.next_link,
        );
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

    // 先收集各文件夹游标与已知 UID（按 sync_enabled / 指定夹）
    let mut requests = Vec::new();
    let mut is_initial = false;
    let folder_ids =
        resolve_mail_folders(db, &cmd.account_id, &cmd.folder_ids, "INBOX").await?;
    {
        let mut conn = db.acquire().await?;
        for folder_id in folder_ids {
            let folder = folder_id.as_str().to_string();
            let prev = folder_states::get(&mut conn, &cmd.account_id, &folder_id)
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

        let folder_id = FolderId::new(result.folder);
        let prev = folder_states::get(&mut tx, &cmd.account_id, &folder_id)
            .await
            .map_err(AppError::from)?;
        let state = merge_folder_progress(
            &cmd.account_id,
            folder_id,
            prev.as_ref(),
            Some(encode_uidvalidity(result.uidvalidity)),
            Some(encode_last_uid(result.last_uid)),
        );
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
        jobs::enqueue_or_reset(conn, account_id, &bundle.message.id, now_ms())
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

    // 投影交由聚合 Worker；同事务入队避免丢任务
    if !message.is_draft {
        jobs::enqueue_or_reset(conn, account_id, &local_id, now_ms())
            .await
            .map_err(AppError::from)?;
    }

    let _ = is_outbound;
    Ok((local_id, None, is_new))
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
                    let mid = existing.message.id.clone();
                    let mut msg = existing.message;
                    msg.sync_status = MessageSyncStatus::DeletedRemote;
                    msg.local_updated_at = now_ms();
                    messages::upsert_with_recipients(
                        &mut tx,
                        &MessageWithRecipients {
                            message: msg,
                            recipients: existing.recipients,
                        },
                    )
                    .await
                    .map_err(AppError::from)?;
                    for cid in conversations::find_conversation_ids_by_message(
                        &mut tx, account_id, &mid,
                    )
                    .await
                    .map_err(AppError::from)?
                    {
                        conversations::recompute_projection(&mut tx, account_id, &cid)
                            .await
                            .map_err(AppError::from)?;
                        changeset.conversation_ids.push(cid);
                    }
                    changeset.message_ids.push(mid);
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
        mail_sync_triggered_at: account.mail_sync_triggered_at,
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

/// 从远端发现文件夹树并 upsert 本地元数据（保留用户勾选与游标）。
pub async fn refresh_mail_folders(
    db: &SqlitePool,
    paths: &AppPaths,
    account_id: &AccountId,
) -> Result<Vec<MailFolderSyncState>, AppError> {
    match tokio::time::timeout(
        FOLDER_REFRESH_TIMEOUT,
        refresh_mail_folders_inner(db, paths, account_id),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err(AppError::from(crate::application::error::PortError::Network {
            message: "拉取文件夹列表超时，请检查网络后重试".into(),
        })),
    }
}

async fn refresh_mail_folders_inner(
    db: &SqlitePool,
    paths: &AppPaths,
    account_id: &AccountId,
) -> Result<Vec<MailFolderSyncState>, AppError> {
    let mut conn = db.acquire().await?;
    let _account = accounts::get(&mut conn, account_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("账户不存在: {account_id}")))?;
    let transport = mail_transports::ensure_default(&mut conn, account_id, now_ms())
        .await
        .map_err(AppError::from)?;
    drop(conn);

    match transport.receive_protocol {
        ReceiveProtocol::Graph => {
            let config = load_azure_auth_config(&paths.config_dir).map_err(AppError::from)?;
            let token =
                graph::auth::get_access_token(&paths.credentials_dir, &config, account_id)
                    .await
                    .map_err(AppError::from)?;
            let remote = graph::mail::list_mail_folders(&token)
                .await
                .map_err(AppError::from)?;
            let mut tx = db.begin().await?;
            let now = now_ms();
            for f in remote {
                let fid = FolderId::new(f.id);
                let parent = f.parent_id.map(FolderId::new);
                let default_enabled = folder_states::default_sync_enabled(
                    fid.as_str(),
                    f.well_known_name.as_deref(),
                );
                folder_states::upsert_discovered(
                    &mut tx,
                    account_id,
                    &fid,
                    parent.as_ref(),
                    &f.display_name,
                    default_enabled,
                    now,
                )
                .await
                .map_err(AppError::from)?;
            }
            tx.commit().await?;
        }
        ReceiveProtocol::Imap => {
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
                .ok_or_else(|| AppError::validation("IMAP 账号缺少 username"))?;
            let secret = credentials::load(&paths.credentials_dir, account_id)
                .await
                .map_err(AppError::from)?
                .ok_or_else(|| AppError::auth("缺少 IMAP 凭证"))?;
            let auth = match transport.auth_kind {
                MailAuthKind::Password => ImapAuth::Password {
                    username: username.clone(),
                    password: secret
                        .password
                        .ok_or_else(|| AppError::auth("缺少 IMAP 密码"))?,
                },
                MailAuthKind::OAuth2 => ImapAuth::OAuth2 {
                    username: username.clone(),
                    access_token: secret
                        .access_token
                        .ok_or_else(|| AppError::auth("缺少 IMAP OAuth access_token"))?,
                },
            };
            let endpoint = ImapEndpoint { host, port, tls };
            let nodes = tokio::task::spawn_blocking(move || {
                let mut session = session_login_blocking(&endpoint, &auth)?;
                let tree = discover_folder_tree(&mut session)?;
                let _ = session.logout();
                Ok::<_, crate::application::error::PortError>(tree)
            })
            .await
            .map_err(|e| AppError::internal(format!("IMAP 文件夹发现失败: {e}")))?
            .map_err(AppError::from)?;

            let mut tx = db.begin().await?;
            let now = now_ms();
            for n in nodes {
                let fid = FolderId::new(n.name.clone());
                let parent = n.parent_name.map(FolderId::new);
                let display =
                    imap_folder_display_name(&n.name, parent.as_ref(), n.delimiter.as_deref());
                let default_enabled = folder_states::default_sync_enabled(fid.as_str(), None);
                folder_states::upsert_discovered(
                    &mut tx,
                    account_id,
                    &fid,
                    parent.as_ref(),
                    &display,
                    default_enabled,
                    now,
                )
                .await
                .map_err(AppError::from)?;
            }
            tx.commit().await?;
        }
    }

    let mut conn = db.acquire().await?;
    folder_states::list_by_account(&mut conn, account_id)
        .await
        .map_err(AppError::from)
}

fn imap_folder_display_name(
    full_name: &str,
    parent: Option<&FolderId>,
    delimiter: Option<&str>,
) -> String {
    if let (Some(parent), Some(d)) = (parent, delimiter) {
        let prefix = format!("{}{}", parent.as_str(), d);
        if let Some(leaf) = full_name.strip_prefix(&prefix) {
            if !leaf.is_empty() {
                return leaf.to_string();
            }
        }
    }
    full_name.to_string()
}

/// 勾选/取消文件夹纳入账户全局同步。
pub async fn set_folder_sync_enabled(
    db: &SqlitePool,
    cmd: SetFolderSyncEnabledCommand,
) -> Result<MailFolderSyncState, AppError> {
    let mut conn = db.acquire().await?;
    folder_states::set_sync_enabled(
        &mut conn,
        &cmd.account_id,
        &cmd.folder_id,
        cmd.sync_enabled,
        now_ms(),
    )
    .await
    .map_err(AppError::from)?;
    folder_states::get(&mut conn, &cmd.account_id, &cmd.folder_id)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| {
            AppError::not_found(format!(
                "文件夹不存在: {}/{}",
                cmd.account_id, cmd.folder_id
            ))
        })
}

