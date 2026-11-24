//! IMAP IDLE 负责收变化通知；账号定时同步独立兜底。

use crate::application::dto::TriggerMailSyncCommand;
use crate::application::use_cases::sync_use_case;
use crate::domain::entities::ReceiveConfig;
use crate::domain::value_objects::{AccountId, FolderId, ReceiveProtocol};
use crate::infrastructure::imap::{IdleOutcome, load_connection, run_idle_until_cancel, session_login};
use crate::infrastructure::paths::AppPaths;
use crate::infrastructure::repository::{accounts, folder_states, mail_transports};
use crate::runtime::{ChangeDispatcher, MailSyncGate};
use sqlx::SqlitePool;
use std::time::Duration;
use super::mail_watchers::WatcherControl;

const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
const BACKFILL_DELAY: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_secs(60);
const RECONNECT_DELAY: Duration = Duration::from_secs(15);

pub(crate) async fn watch_one_account(
    db: SqlitePool,
    paths: AppPaths,
    changes: ChangeDispatcher,
    gate: MailSyncGate,
    account_id: AccountId,
    control: WatcherControl,
) {
    let mut listeners = std::collections::HashMap::new();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut fallback_at = tokio::time::Instant::now();
    while !control.is_stopped() {
        if tokio::time::Instant::now() >= fallback_at {
            let desired = async {
                let mut conn = db.acquire().await.ok()?;
                let transport = mail_transports::get(&mut conn, &account_id).await.ok()??;
                let folders = folder_states::list_sync_enabled(&mut conn, &account_id).await.ok()?;
                Some((transport.imap_idle_supported != Some(false), folders))
            }.await;
            if let Some((supports_idle, folders)) = desired {
                let desired: std::collections::HashMap<_, _> = folders.into_iter()
                    .filter(|f| supports_idle && f.protocol == ReceiveProtocol::Imap && f.watch_enabled)
                    .map(|f| (f.folder_id, f.remote_binding)).collect();
                let retired: Vec<_> = listeners.keys().filter(|key: &&(FolderId, String)| {
                    desired.get(&key.0) != Some(&key.1)
                }).cloned().collect();
                for key in retired {
                    if let Some((listener, handle)) = listeners.remove(&key) {
                        stop_listener(listener, handle).await;
                    }
                }
                for (id, binding) in desired {
                    if control.is_stopped() { break; }
                    let key = (id, binding);
                    listeners.entry(key.clone()).or_insert_with(|| {
                        let listener = WatcherControl::new();
                        let task = tokio::spawn(listen_folder(db.clone(), paths.clone(), account_id.clone(), key, tx.clone(), listener.clone()));
                        (listener, task)
                    });
                }
            }
            sync_folders(&db, &paths, &changes, &gate, &account_id, &control, None).await;
            let backfill = matches!(sync_use_case::has_pending_imap_history(&db, &account_id).await, Ok(true));
            fallback_at = tokio::time::Instant::now() + if backfill { BACKFILL_DELAY } else { POLL_INTERVAL };
        }
        tokio::select! {
            _ = control.cancelled() => break,
            _ = tokio::time::sleep_until(fallback_at) => {},
            Some(key) = rx.recv() => {
                let mut changed = std::collections::HashSet::new();
                if listeners.contains_key(&key) { changed.insert(key.0); }
                while let Ok(key) = rx.try_recv() {
                    if listeners.contains_key(&key) { changed.insert(key.0); }
                }
                if !changed.is_empty() {
                    sync_folders(&db, &paths, &changes, &gate, &account_id, &control, Some(changed.into_iter().collect())).await;
                }
            }
        }
    }
    for (listener, _) in listeners.values() { listener.cancel(); }
    for (_, (listener, handle)) in listeners { stop_listener(listener, handle).await; }
}

async fn stop_listener(listener: WatcherControl, handle: tokio::task::JoinHandle<()>) {
    listener.cancel();
    let _ = handle.await;
}

async fn sync_folders(db: &SqlitePool, paths: &AppPaths, changes: &ChangeDispatcher, gate: &MailSyncGate, account_id: &AccountId, control: &WatcherControl, folder_ids: Option<Vec<FolderId>>) {
    let Some(operation) = control.begin_operation(Duration::from_secs(10 * 60)) else { return; };
    let source = if folder_ids.is_some() { "IDLE 通知" } else { "定时兜底" };
    eprintln!(
        "[{}] IMAP 同步 [{account_id}]: 触发来源={source}",
        chrono::Local::now().format("%F %T%.3f")
    );
    match crate::runtime::mail_sync::trigger_mail_sync_controlled(db, paths, changes, gate, TriggerMailSyncCommand { account_id: account_id.clone(), folder_ids }, operation).await {
        Ok(Some(changeset)) => changes.dispatch(changeset),
        Ok(None) => {},
        Err(error) => eprintln!("[{}] IMAP 同步 [{account_id}]: 来源={source}，{}", chrono::Local::now().format("%F %T%.3f"), error.message),
    }
}

async fn listen_folder(db: SqlitePool, paths: AppPaths, account_id: AccountId, key: (FolderId, String), tx: tokio::sync::mpsc::UnboundedSender<(FolderId, String)>, control: WatcherControl) {
    while !control.is_stopped() {
        let result = async {
            let Some(operation) = control.begin_operation(Duration::from_secs(150)) else { return None; };
            let transport = {
                let mut conn = db.acquire().await.ok()?;
                if !accounts::is_signed_in(&mut conn, &account_id).await.ok()? { return None; }
                mail_transports::get(&mut conn, &account_id).await.ok()??
            };
            let ReceiveConfig::Imap(config) = &transport.receive else { return None; };
            let (endpoint, auth) = load_connection(&paths.credentials_dir, config).await.ok()?;
            let mut session = session_login(&endpoint, &auth, &operation).await.ok()?;
            let binding = key.1.clone();
            operation.run_blocking(move || {
                session.select(&binding).map_err(|error| crate::infrastructure::error::InfraError::Remote {
                    message: format!("IMAP SELECT 失败: {error}"), retryable: true,
                })?;
                run_idle_until_cancel(&mut session, &binding, IDLE_TIMEOUT)
            }).await.ok()
        }.await;
        match result {
            Some(IdleOutcome::Changed) => { if tx.send(key.clone()).is_err() { break; } }
            Some(IdleOutcome::TimedOut) => {},
            _ => control.sleep(RECONNECT_DELAY).await,
        }
    }
}
