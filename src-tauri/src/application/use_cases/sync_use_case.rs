//! 同步用例：按账号协议路由 Graph / IMAP，复用本地仓储与 ChangeSet。

use crate::domain::mail::{DiscoveredFolder, LocationStatus, RemoteFlags, RemoteMessageChange, RemoteMessage};
use crate::application::dto::{
    ChangeOrigin, ChangeSet, ContactSyncProgress, MailSyncProgress, SetFolderSyncEnabledCommand,
    TriggerContactSyncCommand, TriggerMailSyncCommand,
};
use crate::application::error::AppError;
use crate::domain::entities::{
    Contact, MailFolderSyncState, Message, MessageRecipient, MessageWithRecipients, ReceiveConfig, GraphAuthConfig,
};
use crate::domain::services::resolve_self_addresses;
use crate::domain::value_objects::{
    AccountId, ContactId, ContactSource, EmailAddress, FolderId, MessageId,
    ReceiveProtocol, SessionId, SyncStatus, TimestampMs,
};
use crate::infrastructure::config::load_azure_auth_config;
use crate::infrastructure::graph;
use crate::infrastructure::imap::{
    discover_folder_tree, session_login, sync_account_blocking, FolderSyncRequest,
};
use crate::infrastructure::mail::cursor::{encode_uid_cursor, encode_uidvalidity, parse_cursor};
use crate::infrastructure::paths::AppPaths;
use crate::infrastructure::repository::{
    accounts, contacts, folder_states, jobs, mail_transports, messages, mutations,
    sessions,
};
use crate::application::use_cases::aggregate_worker_use_case;
use crate::infrastructure::mail::operation::MailOperation;
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

fn merge_folder_progress(prev: &MailFolderSyncState, sync_cursor: Option<String>, page_cursor: Option<String>) -> MailFolderSyncState {
    let mut state = prev.clone();
    state.sync_cursor = sync_cursor; state.page_cursor = page_cursor; state.synced_at = Some(now_ms());
    state.sync_status = SyncStatus::Idle; state.last_error = None; state.updated_at = now_ms();
    state
}

async fn load_mail_sync_transport(
    conn: &mut sqlx::SqliteConnection,
    account_id: &AccountId,
) -> Result<crate::domain::entities::AccountMailTransport, AppError> {
    match mail_transports::require(conn, account_id).await {
        Ok(transport) => Ok(transport),
        Err(error) => {
            // 同步 IPC 在后台执行；入口失败也必须可由账号状态查询观察。
            accounts::update_mail_sync_meta(conn, account_id, SyncStatus::Error,
                Some(&error.to_string()), None).await?;
            Err(error.into())
        }
    }
}

/// 请求同步指定账户邮件；按 `receive_protocol` 路由。
///
/// Runtime 持有账号 permit 并提供取消/截止时间；本用例不释放或重建同步锁。
pub async fn trigger_mail_sync(
    db: &SqlitePool,
    paths: &AppPaths,
    operation: &MailOperation,
    cmd: TriggerMailSyncCommand,
    on_committed: &(dyn Fn(ChangeSet) + Send + Sync),
) -> Result<Option<ChangeSet>, AppError> {
    operation.checkpoint()?;
    let cmd = &cmd;
    let mut conn = db.acquire().await?;
    let _account = accounts::get(&mut conn, &cmd.account_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("账户不存在: {}", cmd.account_id)))?;
    if !accounts::is_signed_in(&mut conn, &cmd.account_id).await? {
        return Err(AppError::auth("账号已退出，已停止同步"));
    }
    let transport = load_mail_sync_transport(&mut conn, &cmd.account_id).await?;
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

    let result = match &transport.receive {
        ReceiveConfig::Graph(auth) => sync_graph_mail(db, paths, operation, cmd, auth).await,
        ReceiveConfig::Imap(_) => {
            sync_imap_mail(db, paths, operation, cmd, &transport, on_committed).await
        }
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
            // 同步后重入队再排空：修正 In-Reply-To 误挂私聊、同成员自动群复用
            let _ = aggregate_worker_use_case::requeue_account_messages(
                db,
                &cmd.account_id,
                512,
            )
            .await;
            let (_, aggregated) = aggregate_worker_use_case::process_available_jobs(db, 256).await?;
            Ok(merge_changesets(changeset, aggregated))
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
    operation: &MailOperation,
    cmd: &TriggerMailSyncCommand,
    auth: &GraphAuthConfig,
) -> Result<ChangeSet, AppError> {
    let config = load_azure_auth_config(&paths.config_dir).map_err(AppError::from)?;
    let token = operation.wait(graph::auth::get_access_token(&paths.credentials_dir, &config, auth))
        .await
        .map_err(AppError::from)?;

    let folders = resolve_mail_folders(db, &cmd.account_id, &cmd.folder_ids).await?;

    let mut changeset = ChangeSet::empty(cmd.account_id.clone(), ChangeOrigin::IncrementalSync);
    let mut is_initial = false;

    for folder in folders {
        operation.checkpoint()?;
        let prev = {
            let mut conn = db.acquire().await?;
            folder_states::get(&mut conn, &cmd.account_id, &folder)
                .await
                .map_err(AppError::from)?
        }.filter(|state| !state.is_retired && state.protocol == ReceiveProtocol::Graph)
            .ok_or_else(|| AppError::validation("同步文件夹不存在、已退役或协议不匹配"))?;
        if prev.sync_cursor.is_none() {
            is_initial = true;
        }
        let resume = prev.page_cursor.as_deref().or(prev.sync_cursor.as_deref()).map(str::to_owned);

        let page = operation.wait(graph::mail::delta_messages(&token, &folder, &prev.remote_binding, resume.as_deref()))
            .await
            .map_err(AppError::from)?;

        let mut tx = db.begin().await?;
        let current = folder_states::get(&mut tx, &cmd.account_id, &folder).await?
            .filter(|state| !state.is_retired && state.binding_revision == prev.binding_revision)
            .ok_or_else(|| AppError::conflict("文件夹绑定已改变，丢弃旧同步结果"))?;
        for change in page.messages {
            match change {
                RemoteMessageChange::Upsert(remote) => {
                    let can_notify = remote.flags.is_draft == Some(false);
                    let (mid, cid, is_new) =
                        upsert_remote_message(&mut tx, &cmd.account_id, &remote).await?;
                    store_remote_attachments(&mut tx, paths, &cmd.account_id, &mid, &remote.attachments)
                        .await?;
                    changeset.message_ids.push(mid.clone());
                    if let Some(sid) = cid {
                        changeset.session_ids.push(sid);
                    }
                    if is_new && can_notify {
                        changeset.new_incoming_ids.push(mid);
                    }
                }
                RemoteMessageChange::FlagsChanged { remote_key, flags } => {
                    if let Some((mid, sid)) = apply_remote_flags(&mut tx, &cmd.account_id, &folder, &remote_key, flags).await? {
                        changeset.message_ids.push(mid); changeset.session_ids.push(sid);
                    }
                }
                RemoteMessageChange::Remove { remote_key } => {
                    apply_location_removed(&mut tx, &cmd.account_id, &folder, &remote_key, &mut changeset).await?;
                }
            }
        }

        let state = merge_folder_progress(&current, page.delta_link.or_else(|| current.sync_cursor.clone()), page.next_link);
        folder_states::upsert(&mut tx, &state)
            .await
            .map_err(AppError::from)?;
        if !changeset.is_empty() {
            changeset.account_version =
                Some(accounts::bump_change_version(&mut tx, &cmd.account_id).await?);
        }
        operation.checkpoint()?;
        tx.commit().await?;
    }

    if is_initial {
        changeset.origin = ChangeOrigin::InitialSync;
    }
    let mut seen = HashSet::new();
    changeset.session_ids.retain(|c| seen.insert(c.clone()));
    Ok(changeset)
}

/// 合并两个已提交的 ChangeSet；都为空时返回 None。
pub fn merge_changesets(a: ChangeSet, b: Option<ChangeSet>) -> Option<ChangeSet> {
    let Some(b) = b else {
        return (!a.is_empty()).then_some(a);
    };
    let mut out = a;
    out.session_ids.extend(b.session_ids);
    out.message_ids.extend(b.message_ids);
    out.new_incoming_ids.extend(b.new_incoming_ids);
    out.account_cleared |= b.account_cleared;
    let mut seen = HashSet::new();
    out.session_ids.retain(|c| seen.insert(c.clone()));
    let mut seen_msg = HashSet::new();
    out.message_ids.retain(|c| seen_msg.insert(c.clone()));
    (!out.is_empty()).then_some(out)
}

/// 已提交的文件夹进度决定是否继续补拉，不依赖 IDLE 新邮件事件。
pub async fn has_pending_imap_history(
    db: &SqlitePool,
    account_id: &AccountId,
) -> Result<bool, AppError> {
    let mut conn = db.acquire().await?;
    let folders = folder_states::list_sync_enabled(&mut conn, account_id).await?;
    // 夹级错误不参与历史补齐判断，避免坏夹让后台无限回填
    Ok(folders
        .iter()
        .filter(|folder| folder.sync_status != SyncStatus::Error)
        .any(|folder| {
            parse_cursor(folder.sync_cursor.as_deref(), folder.page_cursor.as_deref())
                .is_none_or(|cursor| cursor.low_uid != 1)
        }))
}

async fn sync_imap_mail(
    db: &SqlitePool,
    paths: &AppPaths,
    operation: &MailOperation,
    cmd: &TriggerMailSyncCommand,
    transport: &crate::domain::entities::AccountMailTransport,
    on_committed: &(dyn Fn(ChangeSet) + Send + Sync),
) -> Result<ChangeSet, AppError> {
    let ReceiveConfig::Imap(config) = &transport.receive else { return Err(AppError::validation("账号收信配置不是 IMAP")); };
    let (endpoint, auth) = crate::infrastructure::imap::load_connection(&paths.credentials_dir, config).await?;

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
                .map_err(AppError::from)?.filter(|state| !state.is_retired && state.protocol == ReceiveProtocol::Imap)
                .ok_or_else(|| AppError::validation("同步文件夹不存在、已退役或协议不匹配"))?;
            if prev.sync_cursor.is_none() {
                is_initial = true;
            }
            let known = load_known_remote_messages(&mut conn, &cmd.account_id, &folder).await?;
            let cooling_candidates =
                crate::infrastructure::repository::download_candidates::list_cooling(
                    &mut conn,
                    &cmd.account_id,
                    &folder_id,
                    now_ms(),
                )
                .await
                .map_err(AppError::from)?;
            requests.push(FolderSyncRequest {
                folder_id,
                remote_binding: prev.remote_binding,
                binding_revision: prev.binding_revision,
                stored_generation: prev.generation,
                known_messages: known,
                cooling_candidates,
            });
        }
    }

    let session = session_login(&endpoint, &auth, operation).await.map_err(AppError::from)?;
    let control = operation.clone();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut sync = Box::pin(operation.run_blocking(move || {
        sync_account_blocking(session, &requests, 200, &control, |result| {
            let _ = tx.send(result);
        })
    }));

    let mut changeset = ChangeSet::empty(cmd.account_id.clone(), ChangeOrigin::IncrementalSync);
    let mut first_error: Option<AppError> = None;
    let mut receiving = true;
    // 每个文件夹完成即提交并通知；后一夹失败不会丢弃前面已交付的批次。
    let batch_result = loop {
        tokio::select! {
            result = &mut sync => break result,
            maybe = rx.recv(), if receiving => {
                match maybe {
                    Some(result) => {
                        commit_imap_folder_batch(
                            db, paths, &cmd.account_id, result, &mut changeset, &mut first_error,
                            on_committed,
                        ).await;
                    }
                    None => receiving = false,
                }
            }
        }
    };
    while let Ok(result) = rx.try_recv() {
        commit_imap_folder_batch(
            db, paths, &cmd.account_id, result, &mut changeset, &mut first_error, on_committed,
        ).await;
    }
    let batch = batch_result.map_err(AppError::from)?;

    {
        let mut conn = db.acquire().await?;
        operation.checkpoint()?;
        mail_transports::record_imap_idle_support(&mut conn, transport, batch.idle_supported)
            .await
            .map_err(AppError::from)?;
    }

    if changeset.is_empty() {
        if let Some(error) = first_error {
            return Err(error);
        }
    }

    if is_initial {
        changeset.origin = ChangeOrigin::InitialSync;
    }
    let mut seen = HashSet::new();
    changeset.session_ids.retain(|c| seen.insert(c.clone()));
    Ok(changeset)
}

async fn commit_imap_folder_batch(
    db: &SqlitePool,
    paths: &AppPaths,
    account_id: &AccountId,
    result: crate::infrastructure::imap::FolderSyncResult,
    changeset: &mut ChangeSet,
    first_error: &mut Option<AppError>,
    on_committed: &(dyn Fn(ChangeSet) + Send + Sync),
) {
    match apply_imap_folder_batch(db, paths, account_id, result).await {
        Ok(folder_changes) => {
            if !folder_changes.is_empty() {
                on_committed(folder_changes.clone());
            }
            merge_folder_changes(changeset, folder_changes)
        }
        Err(error) => {
            if first_error.is_none() {
                *first_error = Some(error);
            }
        }
    }
}

/// 合并单个文件夹已提交的变更
fn merge_folder_changes(total: &mut ChangeSet, folder: ChangeSet) {
    total.session_ids.extend(folder.session_ids);
    total.message_ids.extend(folder.message_ids);
    total.new_incoming_ids.extend(folder.new_incoming_ids);
    total.account_version = folder.account_version.or(total.account_version);
}

/// 单夹单批：成功返回该夹已提交的 ChangeSet；失败不提交任何内容。
async fn apply_imap_folder_batch(
    db: &SqlitePool,
    paths: &AppPaths,
    account_id: &AccountId,
    result: crate::infrastructure::imap::FolderSyncResult,
) -> Result<ChangeSet, AppError> {
    // 夹级不可用：记录最后一次可见错误，不推进游标、不影响其他文件夹
    if let Some(folder_error) = result.folder_error.clone() {
        let mut conn = db.acquire().await?;
        folder_states::record_sync_error(&mut conn, account_id, &result.folder_id, &folder_error)
            .await
            .map_err(AppError::from)?;
        return Err(AppError::sync(folder_error));
    }

    let mut changes = ChangeSet::empty(account_id.clone(), ChangeOrigin::IncrementalSync);
    let mut tx = db.begin().await?;
    let current = folder_states::get(&mut tx, account_id, &result.folder_id)
        .await?
        .filter(|state| !state.is_retired && state.binding_revision == result.binding_revision)
        .ok_or_else(|| AppError::conflict("文件夹绑定已改变，丢弃旧同步结果"))?;
    if result.generation_changed {
        invalidate_folder_generation(&mut tx, account_id, &result.folder_id, &mut changes).await?;
    }
    let now = now_ms();
    let generation = result.uidvalidity.to_string();
    for key in &result.discovered_remote_ids {
        crate::infrastructure::repository::download_candidates::record_discovered(
            &mut tx,
            account_id,
            &result.folder_id,
            &crate::domain::value_objects::RemoteKey::new(key),
            result.binding_revision,
            Some(&generation),
            now,
        )
        .await
        .map_err(AppError::from)?;
    }
    for uid in &result.failed_uids {
        let remote_key = crate::infrastructure::imap::imap_remote_id(result.uidvalidity, *uid);
        crate::infrastructure::repository::download_candidates::record_failure(
            &mut tx,
            account_id,
            &result.folder_id,
            &remote_key,
            "本批无法读取或解析正文",
            Some(now + 60_000),
            now,
        )
        .await
        .map_err(AppError::from)?;
    }
    for change in result.changes {
        match change {
            RemoteMessageChange::Upsert(remote) => {
                let can_notify = remote.flags.is_draft == Some(false);
                let (mid, cid, is_new) = upsert_remote_message(&mut tx, account_id, &remote).await?;
                store_remote_attachments(&mut tx, paths, account_id, &mid, &remote.attachments).await?;
                crate::infrastructure::repository::download_candidates::mark_downloaded(
                    &mut tx,
                    account_id,
                    &result.folder_id,
                    &remote.remote_key,
                    now,
                )
                .await
                .map_err(AppError::from)?;
                changes.message_ids.push(mid.clone());
                if let Some(sid) = cid {
                    changes.session_ids.push(sid);
                }
                if is_new && can_notify {
                    changes.new_incoming_ids.push(mid);
                }
            }
            RemoteMessageChange::FlagsChanged { remote_key, flags } => {
                if let Some((mid, sid)) =
                    apply_remote_flags(&mut tx, account_id, &result.folder_id, &remote_key, flags).await?
                {
                    changes.message_ids.push(mid);
                    changes.session_ids.push(sid);
                }
            }
            RemoteMessageChange::Remove { remote_key } => {
                apply_location_removed(&mut tx, account_id, &result.folder_id, &remote_key, &mut changes).await?;
            }
        }
    }
    for key in result.missing_remote_ids {
        apply_location_removed(
            &mut tx,
            account_id,
            &result.folder_id,
            &crate::domain::value_objects::RemoteKey::new(key),
            &mut changes,
        )
        .await?;
    }

    let mut state = merge_folder_progress(
        &current,
        Some(encode_uidvalidity(result.uidvalidity)),
        Some(encode_uid_cursor(result.last_uid, result.low_uid)),
    );
    state.generation = Some(result.uidvalidity.to_string());
    // 本轮成功：清除历史夹级错误
    state.sync_status = SyncStatus::Idle;
    state.last_error = None;
    folder_states::upsert(&mut tx, &state).await.map_err(AppError::from)?;
    if !changes.is_empty() {
        changes.account_version = Some(accounts::bump_change_version(&mut tx, account_id).await?);
    }
    tx.commit().await?;
    Ok(changes)
}

async fn load_known_remote_messages(
    conn: &mut sqlx::SqliteConnection,
    account_id: &AccountId,
    folder: &str,
) -> Result<Vec<(crate::domain::value_objects::RemoteKey, bool)>, AppError> {
    let rows = sqlx::query_as::<_, (String, bool)>(
        "SELECT remote_key, is_read FROM messages WHERE account_id = ? AND folder_id = ? AND remote_key IS NOT NULL AND body_content IS NOT NULL"
    ).bind(account_id.as_str()).bind(folder).fetch_all(conn).await?;
    Ok(rows.into_iter().map(|(key, seen)| (crate::domain::value_objects::RemoteKey::new(key), seen)).collect())
}

async fn upsert_remote_message(
    conn: &mut sqlx::SqliteConnection,
    account_id: &AccountId,
    remote: &RemoteMessage,
) -> Result<(MessageId, Option<SessionId>, bool), AppError> {
    let existing = messages::get_by_location(conn, account_id, remote.folder_id.as_ref().ok_or_else(|| AppError::validation("远端邮件缺少位置"))?, &remote.remote_key)
        .await
        .map_err(AppError::from)?;
    let folder = folder_states::get(conn, account_id, remote.folder_id.as_ref().unwrap()).await?
        .filter(|state| !state.is_retired).ok_or_else(|| AppError::conflict("远端事实指向已失效文件夹"))?;
    if let Some(previous) = &existing {
        if previous.message.location_status == Some(LocationStatus::Invalidated) {
            return Ok((previous.message.id.clone(), None, false));
        }
    }
    let is_new = existing.is_none();
    let local_id = existing
        .as_ref()
        .map(|e| e.message.id.clone())
        .unwrap_or_else(|| MessageId::new(Uuid::new_v4().to_string()));

    let pending = mutations::get_effective(conn, account_id, &local_id)
        .await
        .map_err(AppError::from)?.filter(|pending| pending.is_active);
    let pending_delete = pending
        .as_ref()
        .is_some_and(|p| p.kind == crate::domain::entities::PendingMutationKind::Delete);
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
        .or(remote.flags.is_read)
        .or_else(|| existing.as_ref().and_then(|bundle| bundle.message.remote_is_read))
        .unwrap_or_else(|| existing.as_ref().is_some_and(|bundle| bundle.message.is_read));

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
        location_status: Some(LocationStatus::Present),
        location_revision: existing.as_ref().map(|bundle| bundle.message.location_revision + i64::from(bundle.message.location_status != Some(LocationStatus::Present))).unwrap_or(1),
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
        is_draft: remote.flags.is_draft.or_else(|| existing.as_ref().and_then(|bundle| bundle.message.remote_is_draft))
            .unwrap_or_else(|| existing.as_ref().is_some_and(|bundle| bundle.message.is_draft)),
        remote_is_read: remote.flags.is_read.or_else(|| existing.as_ref().and_then(|bundle| bundle.message.remote_is_read)),
        remote_is_draft: remote.flags.is_draft.or_else(|| existing.as_ref().and_then(|bundle| bundle.message.remote_is_draft)),
        remote_is_deleted: remote.flags.is_deleted.or_else(|| existing.as_ref().and_then(|bundle| bundle.message.remote_is_deleted)),
        is_outbound,
        has_attachments: remote.has_attachments,
        importance: remote.importance,
        folder_id: remote.folder_id.clone(),
        parent_folder_name: Some(folder.display_name.clone()),
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
            send_state: None,
            pending_delete: false,
            folder_role: None,
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

/// 将远端附件写入受管理内容存储并建立消息引用。
///
/// 任一附件写入失败时返回错误并中止本封邮件的附件引用，不留下悬空引用。
async fn store_remote_attachments(
    conn: &mut sqlx::SqliteConnection,
    paths: &AppPaths,
    account_id: &AccountId,
    message_id: &crate::domain::value_objects::MessageId,
    attachments: &[crate::domain::mail::RemoteAttachment],
) -> Result<(), AppError> {
    if attachments.is_empty() {
        return Ok(());
    }
    for (index, att) in attachments.iter().enumerate() {
        let (temp_path, digest) = crate::infrastructure::attachment_store::write_temp(
            &paths.attachments_dir,
            account_id,
            &att.bytes,
        )
        .map_err(AppError::from)?;
        let content_id = format!("att-{}", uuid::Uuid::new_v4().as_hyphenated());
        let installed = crate::infrastructure::attachment_store::install_from_temp(
            &paths.attachments_dir,
            account_id,
            &temp_path,
            &content_id,
            att.bytes.len() as u64,
            &digest,
        )
        .map_err(AppError::from)?;
        let storage_key = installed
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&content_id)
            .to_string();
        crate::infrastructure::repository::attachments::insert_content(
            conn,
            &crate::infrastructure::repository::attachments::AttachmentContentRow {
                content_id: content_id.clone(),
                account_id: account_id.clone(),
                storage_key,
                byte_length: att.bytes.len() as i64,
                digest,
                created_at: now_ms(),
            },
        )
        .await
        .map_err(AppError::from)?;
        crate::infrastructure::repository::attachments::insert_message_attachment(
            conn,
            account_id,
            message_id,
            &crate::infrastructure::repository::attachments::AttachmentRefRow {
                content_id,
                name: att.name.clone(),
                mime: att.mime.clone(),
                sort_order: index as i32,
                byte_length: att.bytes.len() as i64,
            },
        )
        .await
        .map_err(AppError::from)?;
    }
    Ok(())
}

/// 显式稀疏 FLAGS 只改变观测和展示缓存，不触碰正文或会话归属。
async fn apply_remote_flags(
    conn: &mut sqlx::SqliteConnection,
    account_id: &AccountId,
    folder_id: &FolderId,
    remote_key: &crate::domain::value_objects::RemoteKey,
    flags: RemoteFlags,
) -> Result<Option<(MessageId, SessionId)>, AppError> {
    let Some(mut bundle) = messages::get_by_location(conn, account_id, folder_id, remote_key).await? else { return Ok(None) };
    if bundle.message.location_status != Some(LocationStatus::Present) { return Ok(None); }
    let pending = mutations::get_effective(conn, account_id, &bundle.message.id).await?.filter(|pending| pending.is_active);
    let message = &mut bundle.message;
    message.remote_is_read = flags.is_read.or(message.remote_is_read);
    message.remote_is_draft = flags.is_draft.or(message.remote_is_draft);
    message.remote_is_deleted = flags.is_deleted.or(message.remote_is_deleted);
    message.is_read = pending.and_then(|pending| pending.desired_is_read).or(message.remote_is_read).unwrap_or(message.is_read);
    message.is_draft = message.remote_is_draft.unwrap_or(message.is_draft);
    message.local_updated_at = now_ms();
    let result = (message.id.clone(), message.session_id.clone());
    messages::upsert_with_recipients(conn, &bundle).await?;
    sessions::recompute_projection(conn, account_id, &result.1).await?;
    Ok(Some(result))
}

async fn invalidate_folder_generation(
    conn: &mut sqlx::SqliteConnection,
    account_id: &AccountId,
    folder_id: &FolderId,
    changeset: &mut ChangeSet,
) -> Result<(), AppError> {
    let keys = sqlx::query_scalar::<_, String>("SELECT remote_key FROM messages WHERE account_id = ? AND folder_id = ? AND remote_key IS NOT NULL")
        .bind(account_id.as_str()).bind(folder_id.as_str()).fetch_all(&mut *conn).await?;
    for key in keys {
        apply_location_status(conn, account_id, folder_id, &crate::domain::value_objects::RemoteKey::new(key), LocationStatus::Invalidated, changeset).await?;
    }
    Ok(())
}

async fn apply_location_removed(
    conn: &mut sqlx::SqliteConnection,
    account_id: &AccountId,
    folder_id: &FolderId,
    remote_key: &crate::domain::value_objects::RemoteKey,
    changeset: &mut ChangeSet,
) -> Result<(), AppError> {
    apply_location_status(conn, account_id, folder_id, remote_key, LocationStatus::Missing, changeset).await
}

async fn apply_location_status(
    conn: &mut sqlx::SqliteConnection, account_id: &AccountId, folder_id: &FolderId,
    remote_key: &crate::domain::value_objects::RemoteKey, status: LocationStatus, changeset: &mut ChangeSet,
) -> Result<(), AppError> {
    if let Some(existing) = messages::get_by_location(conn, account_id, folder_id, remote_key).await? {
        messages::set_location_status(conn, account_id, &existing.message.id, status).await?;
        mutations::stop_for_message(conn, account_id, &existing.message.id, "远端位置已消失，旧操作已停止").await?;
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
        mail_transports::require(&mut conn, &cmd.account_id)
            .await
            .map_err(AppError::from)?
    };
    let ReceiveConfig::Graph(auth) = &transport.receive else { return Ok(()); };

    let config = load_azure_auth_config(&paths.config_dir).map_err(AppError::from)?;
    let token = graph::auth::get_access_token(&paths.credentials_dir, &config, auth)
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
    operation: &MailOperation,
    account_id: &AccountId,
) -> Result<Vec<MailFolderSyncState>, AppError> {
    operation.checkpoint()?;
    let mut conn = db.acquire().await?;
    let _account = accounts::get(&mut conn, account_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("账户不存在: {account_id}")))?;
    if !accounts::is_signed_in(&mut conn, account_id).await? {
        return Err(AppError::auth("账号已退出，已停止目录发现"));
    }
    let transport = mail_transports::require(&mut conn, account_id)
        .await
        .map_err(AppError::from)?;
    drop(conn);

    let discovered: Vec<DiscoveredFolder> = match &transport.receive {
        ReceiveConfig::Graph(auth) => {
            let config = load_azure_auth_config(&paths.config_dir).map_err(AppError::from)?;
            let token =
                operation.wait(graph::auth::get_access_token(&paths.credentials_dir, &config, auth))
                    .await
                    .map_err(AppError::from)?;
            let remote = operation.wait(graph::mail::list_mail_folders(&token))
                .await
                .map_err(AppError::from)?;
            remote.into_iter().map(|folder| DiscoveredFolder {
                server_role: folder
                    .well_known_name
                    .as_deref()
                    .and_then(graph::mail::role_of_well_known),
                remote_binding: folder.id,
                parent_binding: folder.parent_id,
                display_name: folder.display_name,
            }).collect()
        }
        ReceiveConfig::Imap(config) => {
            let (endpoint, auth) = crate::infrastructure::imap::load_connection(&paths.credentials_dir, config).await?;
            let mut session = session_login(&endpoint, &auth, operation).await.map_err(AppError::from)?;
            let control = operation.clone();
            let nodes = operation.run_blocking(move || {
                control.checkpoint()?;
                let tree = discover_folder_tree(&mut session)?;
                let _ = session.logout();
                Ok::<_, crate::infrastructure::error::InfraError>(tree)
            })
            .await.map_err(AppError::from)?;

            nodes.into_iter().map(|node| {
                let display_name = imap_folder_display_name(&node.name, node.parent_name.as_deref(), node.delimiter.as_deref());
                DiscoveredFolder {
                    server_role: crate::domain::services::role_from_special_use(&node.attributes),
                    remote_binding: if node.name.eq_ignore_ascii_case("INBOX") { "INBOX".into() } else { node.name },
                    parent_binding: node.parent_name.map(|name| if name.eq_ignore_ascii_case("INBOX") { "INBOX".into() } else { name }),
                    display_name,
                }
            }).collect()
        }
    };
    operation.checkpoint()?;
    let mut tx = db.begin().await?;
    let retired = folder_states::reconcile_discovered(&mut tx, account_id, transport.receive.protocol(), &discovered, now_ms()).await?;
    let mut changes = ChangeSet::empty(account_id.clone(), ChangeOrigin::UserAction);
    for folder_id in retired { invalidate_folder_generation(&mut tx, account_id, &folder_id, &mut changes).await?; }
    operation.checkpoint()?;
    tx.commit().await?;

    let mut conn = db.acquire().await?;
    folder_states::list_by_account(&mut conn, account_id)
        .await
        .map_err(AppError::from)
}

fn imap_folder_display_name(
    full_name: &str,
    parent: Option<&str>,
    delimiter: Option<&str>,
) -> String {
    if let (Some(parent), Some(d)) = (parent, delimiter) {
        let prefix = format!("{}{}", parent, d);
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

    #[tokio::test]
    async fn inactive_or_unconfigured_folder_refresh_stops_before_transport_creation_or_authentication() {
        let db = db::open_memory().await.unwrap();
        sqlx::query("INSERT INTO accounts(id,email,is_signed_in,created_at,updated_at) VALUES ('a','a@example.com',0,1,1)")
            .execute(&db).await.unwrap();
        let missing = std::env::temp_dir().join(format!("postlet-not-created-{}", Uuid::new_v4()));
        let paths = AppPaths { config_dir: missing.clone(), local_data_dir: missing.clone(),
            data_dir: missing.clone(), attachments_dir: missing.clone(), credentials_dir: missing.clone(), database_path: missing.join("db") };
        let operation = MailOperation::new(std::time::Duration::from_secs(1));
        let result = refresh_mail_folders(&db, &paths, &operation, &AccountId::new("a")).await;
        assert_eq!(result.unwrap_err().code, crate::application::error::AppErrorCode::Auth);
        sqlx::query("UPDATE accounts SET is_signed_in=1 WHERE id='a'").execute(&db).await.unwrap();
        let result = refresh_mail_folders(&db, &paths, &operation, &AccountId::new("a")).await;
        assert_eq!(result.unwrap_err().code, crate::application::error::AppErrorCode::Conflict);
        assert!(!missing.exists());
        assert_eq!(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM account_mail_transport").fetch_one(&db).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn missing_transport_is_a_visible_sync_error_and_never_creates_graph_configuration() {
        let db = db::open_memory().await.unwrap();
        sqlx::query("INSERT INTO accounts(id,email,created_at,updated_at) VALUES ('a','a@example.com',1,1)").execute(&db).await.unwrap();
        let mut conn = db.acquire().await.unwrap();
        let error = load_mail_sync_transport(&mut conn, &AccountId::new("a")).await.unwrap_err();
        assert_eq!(error.code, crate::application::error::AppErrorCode::Conflict);
        let account = accounts::get(&mut conn, &AccountId::new("a")).await.unwrap().unwrap();
        assert_eq!(account.mail_sync_status, SyncStatus::Error);
        assert!(account.mail_sync_error.unwrap().contains("协议配置缺失"));
        assert!(mail_transports::get(&mut conn, &AccountId::new("a")).await.unwrap().is_none());
    }

    async fn seed_folders(db: &SqlitePool) {
        for (id, binding) in [("inbox", "remote/INBOX"), ("archive", "remote/Archive")] {
            sqlx::query("INSERT INTO mail_folder_sync_state(account_id,folder_id,protocol,remote_binding,display_name,updated_at) SELECT id, ?, 'imap', ?, ?, 1 FROM accounts")
                .bind(id).bind(binding).bind(id).execute(db).await.unwrap();
        }
    }

    fn remote(folder: &str) -> RemoteMessage {
        RemoteMessage {
            attachments: Vec::new(),
            remote_key: RemoteKey::new("same-ordinary-api-id"),
            internet_message_id: Some("<same-rfc-id@example.com>".into()),
            in_reply_to: None, subject: "same subject".into(), body_preview: "same body".into(),
            body_content_type: BodyContentType::Text, body_content: Some("same body".into()),
            from_address: EmailAddress::parse("peer@example.com"), from_name: "Peer".into(),
            recipients: vec![], received_at: Some(1), sent_at: Some(1),
            flags: RemoteFlags { is_read: Some(false), is_draft: Some(false), is_deleted: None }, has_attachments: false, importance: Importance::Normal,
            folder_id: Some(FolderId::new(folder)), parent_folder_name: None, remote_updated_at: None,
        }
    }

    #[tokio::test]
    async fn generation_change_stops_old_targets_and_does_not_inherit_old_delete_or_read_intent() {
        let db = db::open_memory().await.unwrap();
        sqlx::query("INSERT INTO accounts(id,email,created_at,updated_at) VALUES ('a','a@example.com',1,1)").execute(&db).await.unwrap();
        seed_folders(&db).await;
        let a = AccountId::new("a"); let folder = FolderId::new("inbox");
        let mut tx = db.begin().await.unwrap();
        let mut first = remote("inbox"); first.remote_key = RemoteKey::new("imap:1:10");
        let (old_id, _, _) = upsert_remote_message(&mut tx, &a, &first).await.unwrap();
        mutations::put_read(&mut tx, &a, &old_id, true).await.unwrap();
        mutations::put_delete(&mut tx, &a, &old_id).await.unwrap();
        let queued = mutations::get(&mut tx, &a, &old_id).await.unwrap().unwrap();
        assert_eq!(queued.target_folder_id, folder);
        assert_eq!(queued.target_remote_key, first.remote_key);
        let mut changes = ChangeSet::empty(a.clone(), ChangeOrigin::IncrementalSync);
        invalidate_folder_generation(&mut tx, &a, &folder, &mut changes).await.unwrap();
        assert!(mutations::list_due(&mut tx, &a, now_ms(), 32).await.unwrap().is_empty());
        let stopped = mutations::get(&mut tx, &a, &old_id).await.unwrap().unwrap();
        assert!(!stopped.is_active);
        assert!(stopped.last_error.is_some());
        assert_eq!(stopped.target_remote_key, RemoteKey::new("imap:1:10"));
        let mut new_generation = first.clone(); new_generation.remote_key = RemoteKey::new("imap:2:10");
        let (new_id, _, is_new) = upsert_remote_message(&mut tx, &a, &new_generation).await.unwrap();
        assert!(is_new); assert_ne!(old_id, new_id);
        let fresh = messages::get(&mut tx, &a, &new_id).await.unwrap().unwrap();
        assert!(!fresh.message.is_read);
        let old = messages::get(&mut tx, &a, &old_id).await.unwrap().unwrap();
        assert_eq!(old.message.body_content, first.body_content);
        assert_eq!(old.message.location_status, Some(LocationStatus::Invalidated));
        assert!(mutations::get(&mut tx, &a, &new_id).await.unwrap().is_none());
        tx.commit().await.unwrap();
    }

    #[tokio::test]
    async fn flags_preserve_content_and_unknown_values_while_local_intent_overrides_observations() {
        let db = db::open_memory().await.unwrap();
        sqlx::query("INSERT INTO accounts(id,email,created_at,updated_at) VALUES ('a','a@example.com',1,1)").execute(&db).await.unwrap();
        seed_folders(&db).await;
        let a = AccountId::new("a"); let folder = FolderId::new("inbox");
        let mut tx = db.begin().await.unwrap();
        let snapshot = remote("inbox");
        let (id, _, _) = upsert_remote_message(&mut tx, &a, &snapshot).await.unwrap();
        let before = messages::get(&mut tx, &a, &id).await.unwrap().unwrap();
        mutations::put_read(&mut tx, &a, &id, true).await.unwrap();
        apply_remote_flags(&mut tx, &a, &folder, &snapshot.remote_key,
            RemoteFlags { is_read: Some(false), is_draft: Some(true), is_deleted: Some(true) }).await.unwrap();
        let updated = messages::get(&mut tx, &a, &id).await.unwrap().unwrap();
        assert!(updated.message.is_read);
        assert_eq!(updated.message.remote_is_read, Some(false));
        assert_eq!(updated.message.remote_is_deleted, Some(true));
        assert_eq!(updated.message.subject, before.message.subject);
        assert_eq!(updated.message.body_content, before.message.body_content);
        assert_eq!(updated.message.session_id, before.message.session_id);
        assert_eq!(updated.recipients, before.recipients);
        mutations::stop_for_message(&mut tx, &a, &id, "stopped").await.unwrap();
        apply_remote_flags(&mut tx, &a, &folder, &snapshot.remote_key, RemoteFlags::default()).await.unwrap();
        let after = messages::get(&mut tx, &a, &id).await.unwrap().unwrap();
        assert!(!after.message.is_read);
        assert!(after.message.is_draft);
        assert_eq!(after.message.remote_is_deleted, Some(true));
        // A complete snapshot with an empty subject is not a FLAGS placeholder.
        let mut empty_subject = snapshot.clone();
        empty_subject.subject.clear(); empty_subject.body_content = None;
        empty_subject.flags = RemoteFlags::default();
        upsert_remote_message(&mut tx, &a, &empty_subject).await.unwrap();
        let after = messages::get(&mut tx, &a, &id).await.unwrap().unwrap();
        assert_eq!(after.message.subject, "");
        assert_eq!(after.message.body_content, before.message.body_content);
        assert_eq!(after.message.remote_is_deleted, Some(true));
        tx.commit().await.unwrap();
    }

    #[tokio::test]
    async fn same_position_is_idempotent_but_other_folders_and_accounts_are_independent() {
        let db = db::open_memory().await.unwrap();
        sqlx::query("INSERT INTO accounts(id,email,created_at,updated_at) VALUES ('a','a@example.com',1,1),('b','b@example.com',1,1)")
            .execute(&db).await.unwrap();
        seed_folders(&db).await;
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
        assert_eq!(old.message.location_status, Some(LocationStatus::Missing));
        assert_eq!(destination.message.folder_id, Some(FolderId::new("archive")));
        // A source-location removal cannot follow the same key into a new folder.
        assert_eq!(changes.message_ids, vec![original.clone()]);
        let unaffected = messages::get(&mut tx, &b, &other_account).await.unwrap().unwrap();
        // Authoritatively rediscovered same-generation location uses its original entity.
        let (reappeared, _, is_new) = upsert_remote_message(&mut tx, &a, &inbox).await.unwrap();
        assert_eq!(reappeared, original); assert!(!is_new);
        tx.commit().await.unwrap();
    }

    #[tokio::test]
    async fn database_rejects_a_second_entity_for_the_same_position() {
        let db = db::open_memory().await.unwrap();
        sqlx::query("INSERT INTO accounts(id,email,created_at,updated_at) VALUES ('a','a@example.com',1,1)").execute(&db).await.unwrap();
        seed_folders(&db).await;
        let a = AccountId::new("a");
        let mut tx = db.begin().await.unwrap();
        let (id, _, _) = upsert_remote_message(&mut tx, &a, &remote("inbox")).await.unwrap();
        let mut duplicate = messages::get(&mut tx, &a, &id).await.unwrap().unwrap();
        duplicate.message.id = MessageId::new("concurrent-import");
        assert!(messages::upsert_with_recipients(&mut tx, &duplicate).await.is_err());
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages").fetch_one(&mut *tx).await.unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn confirmed_folder_removal_and_same_name_recreation_never_reuse_identity_or_tasks() {
        let db = db::open_memory().await.unwrap();
        sqlx::query("INSERT INTO accounts(id,email,created_at,updated_at) VALUES ('a','a@example.com',1,1),('b','b@example.com',1,1)").execute(&db).await.unwrap();
        let a = AccountId::new("a"); let b = AccountId::new("b");
        let parent = DiscoveredFolder { server_role: None, remote_binding: "Parent".into(), parent_binding: None, display_name: "Parent".into() };
        let child = DiscoveredFolder { server_role: None, remote_binding: "Parent/Box".into(), parent_binding: Some("Parent".into()), display_name: "Box".into() };
        let mut tx = db.begin().await.unwrap();
        folder_states::reconcile_discovered(&mut tx, &a, ReceiveProtocol::Imap, &[parent.clone(), child.clone()], 1).await.unwrap();
        let folders = folder_states::list_by_account(&mut tx, &a).await.unwrap();
        let mut original = folders.iter().find(|folder| folder.remote_binding == "Parent/Box").unwrap().clone();
        let parent_id = folders.iter().find(|folder| folder.remote_binding == "Parent").unwrap().folder_id.clone();
        assert_ne!(original.folder_id.as_str(), "Parent/Box");
        assert_eq!(original.parent_folder_id, Some(parent_id.clone()));
        original.sync_enabled = true; original.generation = Some("1".into());
        original.page_cursor = Some("saved-progress".into());
        folder_states::upsert(&mut tx, &original).await.unwrap();
        let snapshot = remote(original.folder_id.as_str());
        let (old_id, _, _) = upsert_remote_message(&mut tx, &a, &snapshot).await.unwrap();
        mutations::put_read(&mut tx, &a, &old_id, true).await.unwrap();
        let old_task = mutations::get(&mut tx, &a, &old_id).await.unwrap().unwrap();
        assert_eq!(mutations::validate_target(&mut tx, &old_task, ReceiveProtocol::Imap).await.unwrap().as_deref(), Some("Parent/Box"));

        let retired = folder_states::reconcile_discovered(&mut tx, &a, ReceiveProtocol::Imap, &[parent.clone()], 2).await.unwrap();
        assert_eq!(retired, vec![original.folder_id.clone()]);
        let mut changes = ChangeSet::empty(a.clone(), ChangeOrigin::IncrementalSync);
        for folder in &retired { invalidate_folder_generation(&mut tx, &a, folder, &mut changes).await.unwrap(); }
        let retained = messages::get(&mut tx, &a, &old_id).await.unwrap().unwrap();
        assert_eq!(retained.message.body_content, snapshot.body_content);
        assert_eq!(retained.message.location_status, Some(LocationStatus::Invalidated));
        assert_eq!(retained.message.location_revision, 2);
        assert!(mutations::validate_target(&mut tx, &old_task, ReceiveProtocol::Imap).await.unwrap().is_none());
        assert!(upsert_remote_message(&mut tx, &a, &snapshot).await.is_err());

        folder_states::reconcile_discovered(&mut tx, &a, ReceiveProtocol::Imap, &[parent.clone(), child.clone()], 3).await.unwrap();
        let recreated = folder_states::list_by_account(&mut tx, &a).await.unwrap().into_iter().find(|folder| folder.remote_binding == "Parent/Box").unwrap();
        assert_ne!(recreated.folder_id, original.folder_id);
        assert!(!recreated.sync_enabled); assert!(recreated.generation.is_none()); assert!(recreated.page_cursor.is_none());
        assert_eq!(recreated.parent_folder_id, Some(parent_id));
        let (new_id, _, _) = upsert_remote_message(&mut tx, &a, &remote(recreated.folder_id.as_str())).await.unwrap();
        assert_ne!(new_id, old_id);
        let old_folder = folder_states::get(&mut tx, &a, &original.folder_id).await.unwrap().unwrap();
        assert!(old_folder.is_retired); assert_eq!(old_folder.binding_revision, 2);
        let mut illegal_revival = old_folder.clone(); illegal_revival.is_retired = false;
        assert!(folder_states::upsert(&mut tx, &illegal_revival).await.is_err());

        // Another account's same remote name has its own local identity.
        folder_states::reconcile_discovered(&mut tx, &b, ReceiveProtocol::Imap, &[child.clone()], 4).await.unwrap();
        let b_folder = folder_states::list_by_account(&mut tx, &b).await.unwrap().pop().unwrap();
        assert_ne!(b_folder.folder_id, recreated.folder_id);
        let before = folder_states::list_by_account(&mut tx, &a).await.unwrap();
        assert!(folder_states::reconcile_discovered(&mut tx, &a, ReceiveProtocol::Imap, &[child.clone(), child], 5).await.is_err());
        assert_eq!(folder_states::list_by_account(&mut tx, &a).await.unwrap(), before);
        tx.commit().await.unwrap();
    }

    #[tokio::test]
    async fn binding_and_location_revisions_stop_stale_intentions_and_database_prevents_retargeting() {
        let db = db::open_memory().await.unwrap();
        sqlx::query("INSERT INTO accounts(id,email,created_at,updated_at) VALUES ('a','a@example.com',1,1),('b','b@example.com',1,1)").execute(&db).await.unwrap();
        seed_folders(&db).await;
        let a = AccountId::new("a"); let b = AccountId::new("b");
        let mut tx = db.begin().await.unwrap();
        let snapshot = remote("inbox");
        let (id, _, _) = upsert_remote_message(&mut tx, &a, &snapshot).await.unwrap();
        mutations::put_read(&mut tx, &a, &id, true).await.unwrap();
        let target = mutations::get(&mut tx, &a, &id).await.unwrap().unwrap();
        let mut folder = folder_states::get(&mut tx, &a, &FolderId::new("inbox")).await.unwrap().unwrap();
        folder.remote_binding = "confirmed-new-name".into(); folder.binding_revision += 1;
        folder_states::upsert(&mut tx, &folder).await.unwrap();
        assert!(mutations::validate_target(&mut tx, &target, ReceiveProtocol::Imap).await.unwrap().is_none());
        assert!(mutations::get_effective(&mut tx, &a, &id).await.unwrap().is_none());
        upsert_remote_message(&mut tx, &a, &snapshot).await.unwrap();
        assert!(!messages::get(&mut tx, &a, &id).await.unwrap().unwrap().message.is_read);
        mutations::put_read(&mut tx, &a, &id, true).await.unwrap();
        let current = mutations::get(&mut tx, &a, &id).await.unwrap().unwrap();
        assert_eq!(current.target_binding_revision, 2);
        assert_eq!(mutations::validate_target(&mut tx, &current, ReceiveProtocol::Imap).await.unwrap().as_deref(), Some("confirmed-new-name"));
        messages::set_location_status(&mut tx, &a, &id, LocationStatus::Missing).await.unwrap();
        upsert_remote_message(&mut tx, &a, &snapshot).await.unwrap();
        let retained = messages::get(&mut tx, &a, &id).await.unwrap().unwrap();
        assert_eq!(retained.message.location_revision, 3);
        assert!(mutations::validate_target(&mut tx, &current, ReceiveProtocol::Imap).await.unwrap().is_none());
        let mut moved = retained.clone(); moved.message.folder_id = Some(FolderId::new("archive"));
        assert!(messages::upsert_with_recipients(&mut tx, &moved).await.is_err());
        let mut rewound = retained.clone(); rewound.message.location_revision = 1;
        assert!(messages::upsert_with_recipients(&mut tx, &rewound).await.is_err());

        // Null -> first known location is allowed; changing an existing location is not.
        let mut local = retained.clone(); local.message.id = MessageId::new("local-outbound");
        local.message.remote_key = None; local.message.folder_id = None;
        local.message.location_status = None; local.message.location_revision = 0;
        messages::upsert_with_recipients(&mut tx, &local).await.unwrap();
        local.message.remote_key = Some(crate::domain::value_objects::RemoteKey::new("sent-operation-key"));
        local.message.folder_id = Some(FolderId::new("archive"));
        local.message.location_status = Some(LocationStatus::Present); local.message.location_revision = 1;
        messages::upsert_with_recipients(&mut tx, &local).await.unwrap();

        let b_session = aggregate_worker_use_case::ensure_provisional_private(&mut tx, &b, &EmailAddress::parse("peer@example.com").unwrap(), now_ms()).await.unwrap();
        sqlx::query("INSERT INTO mail_folder_sync_state(account_id,folder_id,protocol,remote_binding,display_name,updated_at) VALUES ('a','only-a','imap','only-a-binding','Only A',1)")
            .execute(&mut *tx).await.unwrap();
        let mut cross_account = retained.clone(); cross_account.message.id = MessageId::new("cross-account");
        cross_account.message.account_id = b.clone(); cross_account.message.session_id = b_session;
        cross_account.message.folder_id = Some(FolderId::new("only-a"));
        assert!(messages::upsert_with_recipients(&mut tx, &cross_account).await.is_err());
        messages::set_location_status(&mut tx, &a, &id, LocationStatus::Invalidated).await.unwrap();
        let mut forbidden = retained; forbidden.message.location_revision = 5;
        assert!(messages::upsert_with_recipients(&mut tx, &forbidden).await.is_err());
        tx.commit().await.unwrap();
    }
}

