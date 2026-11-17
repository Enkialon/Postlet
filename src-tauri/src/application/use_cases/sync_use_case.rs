//! 同步用例：按账号协议路由 Graph / IMAP，复用本地仓储与 ChangeSet。

use crate::domain::mail::{RemoteMessageChange, RemoteMessage};
use crate::application::dto::{
    ChangeOrigin, ChangeSet, ContactSyncProgress, MailSyncProgress, SetFolderSyncEnabledCommand,
    TriggerContactSyncCommand, TriggerMailSyncCommand,
};
use crate::application::error::AppError;
use crate::domain::entities::{
    Contact, MailFolderSyncState, Message, MessageRecipient, MessageWithRecipients,
};
use crate::domain::services::resolve_self_addresses;
use crate::domain::value_objects::{
    AccountId, ContactId, ContactSource, EmailAddress, FolderId, MailAuthKind, MailTlsMode,
    MessageId, MessageSyncStatus, ReceiveProtocol, SessionId, SyncStatus, TimestampMs,
};
use crate::infrastructure::config::load_azure_auth_config;
use crate::infrastructure::graph;
use crate::infrastructure::imap::{
    discover_folder_tree, session_login_blocking, sync_account_blocking, FolderSyncRequest,
    ImapAuth, ImapEndpoint,
};
use crate::infrastructure::mail::credentials;
use crate::infrastructure::mail::cursor::{encode_uid_cursor, encode_uidvalidity, parse_cursor};
use crate::infrastructure::paths::AppPaths;
use crate::infrastructure::repository::{
    accounts, contacts, folder_states, jobs, mail_transports, messages, mutations,
    sessions,
};
use crate::application::use_cases::aggregate_worker_use_case;
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

/// `folder_ids` 非空时按指定夹；否则只同步已勾选夹。无勾选则空列表（不偷偷回落收件箱）。
async fn resolve_mail_folders(
    db: &SqlitePool,
    account_id: &AccountId,
    folder_ids: &Option<Vec<FolderId>>,
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
    Ok(enabled.into_iter().map(|s| s.folder_id).collect())
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
        sync_enabled: prev.map(|p| p.sync_enabled).unwrap_or(false),
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
            // 同步后重入队再排空：修正 In-Reply-To 误挂私聊、同成员自动群复用
            let _ = aggregate_worker_use_case::requeue_account_messages(
                db,
                &cmd.account_id,
                512,
            )
            .await;
            let _ =
                aggregate_worker_use_case::process_available_jobs(db, Some(changes), 256).await;
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

    let folders = resolve_mail_folders(db, &cmd.account_id, &cmd.folder_ids).await?;

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
                RemoteMessageChange::Upsert(remote) => {
                    let is_draft = remote.is_draft;
                    let (mid, cid, is_new) =
                        upsert_remote_message(&mut tx, &cmd.account_id, &remote).await?;
                    changeset.message_ids.push(mid.clone());
                    if let Some(sid) = cid {
                        changeset.session_ids.push(sid);
                    }
                    if is_new && !is_draft {
                        changeset.new_incoming_ids.push(mid);
                    }
                }
                RemoteMessageChange::Remove { remote_key } => {
                    apply_location_removed(&mut tx, &cmd.account_id, &folder, &remote_key, &mut changeset).await?;
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

    if is_initial {
        changeset.origin = ChangeOrigin::InitialSync;
    }
    let mut seen = HashSet::new();
    changeset.session_ids.retain(|c| seen.insert(c.clone()));
    Ok(changeset)
}

/// 已提交的文件夹进度决定是否继续补拉，不依赖 IDLE 新邮件事件。
pub async fn has_pending_imap_history(
    db: &SqlitePool,
    account_id: &AccountId,
) -> Result<bool, AppError> {
    let mut conn = db.acquire().await?;
    let folders = folder_states::list_sync_enabled(&mut conn, account_id).await?;
    Ok(folders.iter().any(|folder| {
        parse_cursor(folder.delta_link.as_deref(), folder.next_link.as_deref())
            .is_none_or(|cursor| cursor.low_uid != 1)
    }))
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
    let secret = credentials::load(&paths.credentials_dir, &transport)
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
    let folder_ids = resolve_mail_folders(db, &cmd.account_id, &cmd.folder_ids).await?;
    if folder_ids.is_empty() {
        return Err(AppError::validation(
            "未勾选任何同步文件夹，请先在设置中勾选收件箱等文件夹后再同步",
        ));
    }
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
                RemoteMessageChange::Upsert(remote) => {
                    let is_draft = remote.is_draft;
                    let (mid, cid, is_new) =
                        upsert_remote_message(&mut tx, &cmd.account_id, &remote).await?;
                    changeset.message_ids.push(mid.clone());
                    if let Some(sid) = cid {
                        changeset.session_ids.push(sid);
                    }
                    if is_new && !is_draft {
                        changeset.new_incoming_ids.push(mid);
                    }
                }
                RemoteMessageChange::Remove { remote_key } => {
                    apply_location_removed(&mut tx, &cmd.account_id, &FolderId::new(&result.folder), &remote_key, &mut changeset).await?;
                }
            }
        }
        for key in result.missing_remote_ids {
            apply_location_removed(&mut tx, &cmd.account_id, &FolderId::new(&result.folder), &crate::domain::value_objects::RemoteKey::new(key), &mut changeset).await?;
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
            Some(encode_uid_cursor(result.last_uid, result.low_uid)),
        );
        folder_states::upsert(&mut tx, &state)
            .await
            .map_err(AppError::from)?;
        tx.commit().await?;
    }

    if is_initial {
        changeset.origin = ChangeOrigin::InitialSync;
    }
    let mut seen = HashSet::new();
    changeset.session_ids.retain(|c| seen.insert(c.clone()));
    Ok(changeset)
}

async fn load_known_uids(
    conn: &mut sqlx::SqliteConnection,
    account_id: &AccountId,
    folder: &str,
) -> Result<Vec<(u32, bool)>, AppError> {
    let prefix = format!("imap:{folder}:");
    // 本地软删也是已经下载过的 UID，不能因其仍存在远端而无限补抓。
    let rows = sqlx::query_as::<_, (String, i64)>(
        "SELECT remote_key, is_read FROM messages
         WHERE account_id = ? AND folder_id = ? AND remote_key LIKE ? AND body_content IS NOT NULL",
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
) -> Result<(MessageId, Option<SessionId>, bool), AppError> {
    let existing = messages::get_by_location(conn, account_id, remote.folder_id.as_ref().ok_or_else(|| AppError::validation("远端邮件缺少位置"))?, &remote.remote_key)
        .await
        .map_err(AppError::from)?;
    let is_new = existing.is_none();
    let local_id = existing
        .as_ref()
        .map(|e| e.message.id.clone())
        .unwrap_or_else(|| MessageId::new(Uuid::new_v4().to_string()));

    let pending = mutations::get(conn, account_id, &local_id)
        .await
        .map_err(AppError::from)?;
    let pending_delete = pending
        .as_ref()
        .is_some_and(|p| p.kind == crate::application::dto::PendingMutationKind::Delete);
    // 用户已删或待推删除：不复活
    if pending_delete {
        if let Some(bundle) = existing {
            return Ok((bundle.message.id, None, false));
        }
    }

    // FLAGS-only 占位：空 subject 且无 body → 仅更新已读（有 pending 时保留本地）
    if !is_new && remote.subject.is_empty() && remote.body_content.is_none() {
        let mut bundle = existing.unwrap();
        if let Some(p) = &pending {
            if let Some(desired) = p.desired_is_read {
                bundle.message.is_read = desired;
            }
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
        return Ok((bundle.message.id, Some(bundle.message.session_id), false));
    }

    let account = accounts::get(conn, account_id)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::not_found(format!("账户不存在: {account_id}")))?;
    let self_set = resolve_self_addresses(&account);
    let is_outbound = match &remote.from_address {
        Some(from) => self_set.contains(from),
        _ => false,
    };

    let is_read = pending
        .as_ref()
        .and_then(|p| p.desired_is_read)
        .unwrap_or(remote.is_read);

    let body_content = if let Some(ref ex) = existing {
        remote
            .body_content
            .clone()
            .or_else(|| ex.message.body_content.clone())
    } else {
        remote.body_content.clone()
    };

    let now = now_ms();
    // 已有保留 session；新建则按父信 / 对端地址建临时私聊 Session
    let session_id = if let Some(ref ex) = existing {
        ex.message.session_id.clone()
    } else {
        provisional_session_id(conn, account_id, remote, &account.email, is_outbound, now).await?
    };

    let message = Message {
        id: local_id.clone(),
        account_id: account_id.clone(),
        session_id: session_id.clone(),
        remote_key: Some(remote.remote_key.clone()),
        internet_message_id: remote.internet_message_id.clone(),
        in_reply_to: remote.in_reply_to.clone(),
        subject: remote.subject.clone(),
        body_preview: remote.body_preview.clone(),
        body_content_type: remote.body_content_type,
        body_content,
        from_address: remote.from_address.clone(),
        from_name: remote.from_name.clone(),
        received_at: remote.received_at.or(Some(now)),
        sent_at: remote.sent_at,
        is_read,
        is_draft: remote.is_draft,
        is_outbound,
        has_attachments: remote.has_attachments,
        importance: remote.importance,
        folder_id: remote.folder_id.clone(),
        parent_folder_name: remote.parent_folder_name.clone(),
        sync_status: MessageSyncStatus::Synced,
        local_updated_at: now,
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
        jobs::enqueue_or_reset(conn, account_id, &local_id, now)
            .await
            .map_err(AppError::from)?;
    }

    Ok((local_id, Some(session_id), is_new))
}

/// 移出事实只作用于来源位置；不追踪远端 ID、不替换为其他文件夹位置。
async fn apply_location_removed(
    conn: &mut sqlx::SqliteConnection,
    account_id: &AccountId,
    folder_id: &FolderId,
    remote_key: &crate::domain::value_objects::RemoteKey,
    changeset: &mut ChangeSet,
) -> Result<(), AppError> {
    if let Some(existing) = messages::get_by_location(conn, account_id, folder_id, remote_key).await? {
        messages::mark_deleted_remote(conn, account_id, &[existing.message.id.clone()]).await?;
        sessions::recompute_projection(conn, account_id, &existing.message.session_id).await?;
        changeset.message_ids.push(existing.message.id);
        changeset.session_ids.push(existing.message.session_id);
    }
    Ok(())
}

/// 同步落库前的临时 Session：父信优先，否则对端私聊，否则本账户地址。
async fn provisional_session_id(
    conn: &mut sqlx::SqliteConnection,
    account_id: &AccountId,
    remote: &RemoteMessage,
    self_email: &EmailAddress,
    is_outbound: bool,
    now: TimestampMs,
) -> Result<SessionId, AppError> {
    if let Some(irt) = remote
        .in_reply_to
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let first = irt.split_whitespace().next().unwrap_or(irt).trim();
        if let Some(parent) = messages::find_by_internet_message_id(conn, account_id, first)
            .await
            .map_err(AppError::from)?
        {
            if let Some(root) =
                sessions::resolve_root(conn, account_id, &parent.message.session_id)
                    .await
                    .map_err(AppError::from)?
            {
                return Ok(root.id);
            }
        }
    }

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
            .or_else(|| remote.from_address.clone())
    } else {
        remote.from_address.clone()
    };
    let address = peer.unwrap_or_else(|| self_email.clone());
    aggregate_worker_use_case::ensure_provisional_private(conn, account_id, &address, now).await
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
                c.source = ContactSource::Graph;
                c.updated_at = now;
                c
            } else {
                Contact {
                    id: ContactId::new(Uuid::new_v4().to_string()),
                    account_id: cmd.account_id.clone(),
                    graph_id: Some(remote.graph_id),
                    display_name: remote.display_name,
                    normalized_address: remote.normalized_address,
                    source: ContactSource::Graph,
                    created_at: now,
                    updated_at: now,
                }
            };
            contacts::upsert(&mut tx, &contact)
                .await
                .map_err(AppError::from)?;
        }
        for gid in page.removed_graph_ids {
            if let Some(mut c) = contacts::get_by_graph_id(&mut tx, &cmd.account_id, &gid)
                .await
                .map_err(AppError::from)?
            {
                // 移出通讯录 ≠ 删人：降为 observed，保留短名给会话
                c.graph_id = None;
                c.source = ContactSource::Observed;
                c.updated_at = now_ms();
                contacts::upsert(&mut tx, &c)
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
            let secret = credentials::load(&paths.credentials_dir, &transport)
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

#[cfg(test)]
mod location_tests {
    use super::*;
    use crate::domain::value_objects::{BodyContentType, Importance, RemoteKey};
    use crate::infrastructure::db;

    fn remote(folder: &str) -> RemoteMessage {
        RemoteMessage {
            remote_key: RemoteKey::new("same-ordinary-api-id"),
            internet_message_id: Some("<same-rfc-id@example.com>".into()),
            in_reply_to: None, subject: "same subject".into(), body_preview: "same body".into(),
            body_content_type: BodyContentType::Text, body_content: Some("same body".into()),
            from_address: EmailAddress::parse("peer@example.com"), from_name: "Peer".into(),
            recipients: vec![], received_at: Some(1), sent_at: Some(1), is_read: false,
            is_draft: false, has_attachments: false, importance: Importance::Normal,
            folder_id: Some(FolderId::new(folder)), parent_folder_name: None, remote_updated_at: None,
        }
    }

    #[tokio::test]
    async fn same_position_is_idempotent_but_other_folders_and_accounts_are_independent() {
        let db = db::open_memory().await.unwrap();
        sqlx::query("INSERT INTO accounts(id,email,created_at,updated_at) VALUES ('a','a@example.com',1,1),('b','b@example.com',1,1)")
            .execute(&db).await.unwrap();
        let a = AccountId::new("a"); let b = AccountId::new("b");
        let mut tx = db.begin().await.unwrap();
        let inbox = remote("inbox");
        let (original, _, is_new) = upsert_remote_message(&mut tx, &a, &inbox).await.unwrap();
        assert!(is_new);
        let (repeat, _, is_new) = upsert_remote_message(&mut tx, &a, &inbox).await.unwrap();
        assert_eq!(original, repeat); assert!(!is_new);
        let (other_folder, _, _) = upsert_remote_message(&mut tx, &a, &remote("archive")).await.unwrap();
        let (other_account, _, _) = upsert_remote_message(&mut tx, &b, &inbox).await.unwrap();
        assert_ne!(original, other_folder); assert_ne!(original, other_account);
        let mut changes = ChangeSet::empty(a.clone(), ChangeOrigin::IncrementalSync);
        apply_location_removed(&mut tx, &a, &FolderId::new("inbox"), &inbox.remote_key, &mut changes).await.unwrap();
        let old = messages::get(&mut tx, &a, &original).await.unwrap().unwrap();
        let destination = messages::get(&mut tx, &a, &other_folder).await.unwrap().unwrap();
        assert_eq!(old.message.body_content.as_deref(), Some("same body"));
        assert_eq!(old.message.sync_status, MessageSyncStatus::DeletedRemote);
        assert_eq!(destination.message.sync_status, MessageSyncStatus::Synced);
        assert_eq!(destination.message.folder_id, Some(FolderId::new("archive")));
        // A source-location removal cannot follow the same key into a new folder.
        assert_eq!(changes.message_ids, vec![original.clone()]);
        let unaffected = messages::get(&mut tx, &b, &other_account).await.unwrap().unwrap();
        assert_eq!(unaffected.message.sync_status, MessageSyncStatus::Synced);
        // Authoritatively rediscovered same-generation location uses its original entity.
        let (reappeared, _, is_new) = upsert_remote_message(&mut tx, &a, &inbox).await.unwrap();
        assert_eq!(reappeared, original); assert!(!is_new);
        tx.commit().await.unwrap();
    }

    #[tokio::test]
    async fn database_rejects_a_second_entity_for_the_same_position() {
        let db = db::open_memory().await.unwrap();
        sqlx::query("INSERT INTO accounts(id,email,created_at,updated_at) VALUES ('a','a@example.com',1,1)").execute(&db).await.unwrap();
        let a = AccountId::new("a");
        let mut tx = db.begin().await.unwrap();
        let (id, _, _) = upsert_remote_message(&mut tx, &a, &remote("inbox")).await.unwrap();
        let mut duplicate = messages::get(&mut tx, &a, &id).await.unwrap().unwrap();
        duplicate.message.id = MessageId::new("concurrent-import");
        assert!(messages::upsert_with_recipients(&mut tx, &duplicate).await.is_err());
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages").fetch_one(&mut *tx).await.unwrap();
        assert_eq!(count, 1);
    }
}

