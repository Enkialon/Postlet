//! IMAP 监听任务表：配置变化先取消并等待旧任务，不遗留永久 started 标记。

use crate::application::error::AppError;
use crate::domain::entities::AccountMailTransport;
use crate::domain::value_objects::{AccountId, ReceiveProtocol};
use crate::infrastructure::mail::operation::MailOperation;
use crate::infrastructure::paths::AppPaths;
use crate::infrastructure::repository::{accounts, mail_transports};
use crate::runtime::{ChangeDispatcher, MailSyncGate};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;

#[derive(Default)]
struct ControlState {
    stopped: bool,
    operation: Option<MailOperation>,
}

#[derive(Clone)]
pub(crate) struct WatcherControl {
    state: Arc<Mutex<ControlState>>,
    stop: watch::Sender<bool>,
}

impl WatcherControl {
    fn new() -> Self {
        let (stop, _) = watch::channel(false);
        Self {
            state: Arc::new(Mutex::new(ControlState::default())),
            stop,
        }
    }
    pub fn is_stopped(&self) -> bool {
        *self.stop.borrow()
    }
    pub fn begin_operation(&self, budget: Duration) -> Option<MailOperation> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.stopped {
            return None;
        }
        let operation = MailOperation::new(budget);
        state.operation = Some(operation.clone());
        Some(operation)
    }
    pub fn cancel(&self) {
        let operation = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.stopped = true;
            state.operation.take()
        };
        self.stop.send_replace(true);
        if let Some(operation) = operation {
            operation.cancel();
        }
    }
    pub async fn cancelled(&self) {
        let mut receiver = self.stop.subscribe();
        let stopped = *receiver.borrow_and_update();
        if !stopped {
            let _ = receiver.changed().await;
        }
    }
    pub async fn sleep(&self, duration: Duration) {
        tokio::select! { _ = self.cancelled() => {}, _ = tokio::time::sleep(duration) => {} }
    }
}

struct WatchTask {
    config: AccountMailTransport,
    control: WatcherControl,
    handle: Option<JoinHandle<()>>,
}
impl WatchTask {
    async fn stop(mut self) {
        self.control.cancel();
        if let Some(handle) = self.handle.take() {
            if let Err(error) = handle.await {
                eprintln!("IMAP 监听退出异常: {error}");
            }
        }
    }
}
impl Drop for WatchTask {
    fn drop(&mut self) {
        self.control.cancel();
    }
}

fn same_receive_config(a: &AccountMailTransport, b: &AccountMailTransport) -> bool {
    a.account_id == b.account_id
        && a.receive_protocol == b.receive_protocol
        && a.auth_kind == b.auth_kind
        && a.credential_ref == b.credential_ref
        && a.imap_host == b.imap_host
        && a.imap_port == b.imap_port
        && a.imap_tls == b.imap_tls
        && a.username == b.username
}

async fn reconcile(
    tasks: &mut HashMap<String, WatchTask>,
    configs: Vec<AccountMailTransport>,
    paused: &HashMap<String, String>,
    mut spawn: impl FnMut(AccountMailTransport, WatcherControl) -> JoinHandle<()>,
) {
    let desired: HashMap<_, _> = configs
        .into_iter()
        .filter(|config| config.receive_protocol == ReceiveProtocol::Imap)
        .map(|config| (config.account_id.to_string(), config))
        .collect();
    let obsolete: Vec<_> = tasks
        .iter()
        .filter(|(id, task)| {
            paused.contains_key(*id)
                || task.handle.as_ref().is_none_or(JoinHandle::is_finished)
                || desired
                    .get(*id)
                    .is_none_or(|config| !same_receive_config(config, &task.config))
        })
        .map(|(id, _)| id.clone())
        .collect();
    for id in obsolete {
        if let Some(task) = tasks.remove(&id) {
            task.stop().await;
        }
    }
    for (id, config) in desired {
        if paused.contains_key(&id) || tasks.contains_key(&id) {
            continue;
        }
        let control = WatcherControl::new();
        let handle = spawn(config.clone(), control.clone());
        tasks.insert(
            id,
            WatchTask {
                config,
                control,
                handle: Some(handle),
            },
        );
    }
}

enum Command {
    Pause {
        account: String,
        ticket: String,
        reply: oneshot::Sender<Result<(), AppError>>,
    },
    Resume {
        account: String,
        ticket: String,
    },
    Changed(String),
}

#[derive(Clone)]
pub struct MailWatchers {
    sender: mpsc::UnboundedSender<Command>,
}

pub struct WatcherPause {
    watchers: MailWatchers,
    account: String,
    ticket: String,
}
impl Drop for WatcherPause {
    fn drop(&mut self) {
        let _ = self.watchers.sender.send(Command::Resume {
            account: self.account.clone(),
            ticket: self.ticket.clone(),
        });
    }
}

impl MailWatchers {
    pub fn spawn(
        db: SqlitePool,
        paths: AppPaths,
        changes: ChangeDispatcher,
        gate: MailSyncGate,
    ) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let query_db = db.clone();
        tauri::async_runtime::spawn(manage(
            receiver,
            move || {
                let db = query_db.clone();
                async move { active_configs(&db).await }
            },
            move |config, control| {
                tokio::spawn(super::mail_watcher::watch_one_account(
                    db.clone(),
                    paths.clone(),
                    changes.clone(),
                    gate.clone(),
                    config.account_id,
                    control,
                ))
            },
        ));
        Self { sender }
    }

    pub async fn pause(&self, account: &AccountId) -> Result<WatcherPause, AppError> {
        let (reply, response) = oneshot::channel();
        let guard = WatcherPause {
            watchers: self.clone(),
            account: account.to_string(),
            ticket: uuid::Uuid::new_v4().to_string(),
        };
        self.sender
            .send(Command::Pause {
                account: guard.account.clone(),
                ticket: guard.ticket.clone(),
                reply,
            })
            .map_err(|_| AppError::internal("邮件监听管理器已退出"))?;
        response
            .await
            .map_err(|_| AppError::internal("邮件监听管理器未确认退出"))??;
        Ok(guard)
    }

    pub fn changed(&self, account: &AccountId) {
        let _ = self.sender.send(Command::Changed(account.to_string()));
    }
}

async fn manage<Fut>(
    mut receiver: mpsc::UnboundedReceiver<Command>,
    mut load: impl FnMut() -> Fut,
    mut spawn: impl FnMut(AccountMailTransport, WatcherControl) -> JoinHandle<()>,
) where
    Fut: std::future::Future<Output = Result<Vec<AccountMailTransport>, AppError>>,
{
    let mut tasks = HashMap::<String, WatchTask>::new();
    let mut paused = HashMap::<String, String>::new();
    loop {
        // 只有完整查询成功才退役缺失账号；数据库错误不能被解释为退出。
        if let Ok(configs) = load().await {
            reconcile(&mut tasks, configs, &paused, &mut spawn).await;
        }
        let command = tokio::select! {
            command = receiver.recv() => command,
            _ = tokio::time::sleep(Duration::from_secs(5)) => continue,
        };
        match command {
            Some(Command::Pause {
                account,
                ticket,
                reply,
            }) => {
                if paused.contains_key(&account) {
                    let _ = reply.send(Err(AppError::conflict("账号监听正在变更")));
                    continue;
                }
                paused.insert(account.clone(), ticket);
                if let Some(task) = tasks.remove(&account) {
                    task.stop().await;
                }
                let _ = reply.send(Ok(()));
            }
            Some(Command::Resume { account, ticket }) => {
                if paused.get(&account) == Some(&ticket) {
                    paused.remove(&account);
                }
            }
            Some(Command::Changed(account)) => {
                if let Some(task) = tasks.remove(&account) {
                    task.stop().await;
                }
            }
            None => {
                for (_, task) in tasks.drain() {
                    task.stop().await;
                }
                break;
            }
        }
    }
}

async fn active_configs(db: &SqlitePool) -> Result<Vec<AccountMailTransport>, AppError> {
    let mut conn = db.acquire().await?;
    let mut configs = Vec::new();
    for account in accounts::list(&mut conn).await? {
        if let Some(config) = mail_transports::get(&mut conn, &account.id).await? {
            configs.push(config);
        }
    }
    Ok(configs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::value_objects::{MailAuthKind, MailTlsMode};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn config() -> AccountMailTransport {
        let mut config = AccountMailTransport::graph_default(AccountId::new("a"), 1);
        config.receive_protocol = ReceiveProtocol::Imap;
        config.auth_kind = MailAuthKind::Password;
        config.credential_ref = Some("reference-one".into());
        config.imap_host = Some("imap.example.com".into());
        config.imap_port = Some(993);
        config.imap_tls = Some(MailTlsMode::Implicit);
        config.username = Some("user".into());
        config
    }

    #[tokio::test]
    async fn config_changes_logout_and_relogin_reconcile_without_duplicate_or_orphan_tasks() {
        let starts = Arc::new(AtomicUsize::new(0));
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let factory = |_: AccountMailTransport, control: WatcherControl| {
            starts.fetch_add(1, Ordering::SeqCst);
            let active = active.clone();
            let max_active = max_active.clone();
            tokio::spawn(async move {
                let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                max_active.fetch_max(count, Ordering::SeqCst);
                control.cancelled().await;
                active.fetch_sub(1, Ordering::SeqCst);
            })
        };
        let mut tasks = HashMap::new();
        let paused = HashMap::new();
        let original = config();
        reconcile(&mut tasks, vec![original.clone()], &paused, factory).await;
        for index in 0..10 {
            let mut observation = original.clone();
            observation.imap_idle_supported = Some(true);
            observation.updated_at = index;
            observation.smtp_host = Some("new-smtp.example.com".into());
            reconcile(&mut tasks, vec![observation], &paused, factory).await;
        }
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        let mut rotated = original.clone();
        rotated.credential_ref = Some("reference-two".into());
        reconcile(&mut tasks, vec![rotated.clone()], &paused, factory).await;
        assert_eq!(starts.load(Ordering::SeqCst), 2);
        let mut graph = rotated.clone();
        graph.receive_protocol = ReceiveProtocol::Graph;
        reconcile(&mut tasks, vec![graph], &paused, factory).await;
        assert!(tasks.is_empty());
        assert_eq!(active.load(Ordering::SeqCst), 0);
        reconcile(&mut tasks, vec![rotated.clone()], &paused, factory).await;
        reconcile(&mut tasks, vec![], &paused, factory).await; // 已退出，不在活动账号列表
        assert_eq!(active.load(Ordering::SeqCst), 0);
        reconcile(&mut tasks, vec![rotated.clone()], &paused, factory).await; // 再登录
        reconcile(&mut tasks, vec![rotated], &paused, factory).await;
        assert_eq!(starts.load(Ordering::SeqCst), 4);
        reconcile(&mut tasks, vec![], &paused, factory).await;
        assert!(tasks.is_empty());
        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert_eq!(max_active.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn replacement_waits_for_old_worker_cleanup_and_stop_prevents_a_late_connection() {
        let starts = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let factory = |_: AccountMailTransport, control: WatcherControl| {
            starts.fetch_add(1, Ordering::SeqCst);
            let release = release.clone();
            tokio::spawn(async move {
                control.cancelled().await;
                release.acquire_owned().await.unwrap().forget();
            })
        };
        let mut tasks = HashMap::new();
        let paused = HashMap::new();
        let original = config();
        reconcile(&mut tasks, vec![original.clone()], &paused, factory).await;
        let old_control = tasks["a"].control.clone();
        let operation = old_control.begin_operation(Duration::from_secs(5)).unwrap();
        let mut changed = original;
        changed.credential_ref = Some("new-reference".into());
        {
            let replacement = reconcile(&mut tasks, vec![changed], &paused, factory);
            tokio::pin!(replacement);
            assert!(
                tokio::time::timeout(Duration::from_millis(20), &mut replacement)
                    .await
                    .is_err()
            );
            assert!(old_control.is_stopped());
            assert!(operation.checkpoint().is_err());
            assert!(
                old_control
                    .begin_operation(Duration::from_secs(5))
                    .is_none()
            );
            assert_eq!(starts.load(Ordering::SeqCst), 1);
            release.add_permits(1);
            replacement.await;
        }
        assert_eq!(starts.load(Ordering::SeqCst), 2);
        release.add_permits(1);
        reconcile(&mut tasks, vec![], &paused, factory).await;
    }

    #[tokio::test]
    async fn pause_stops_listener_and_same_config_can_restart_after_clear_or_unexpected_exit() {
        let mut tasks = HashMap::new();
        let mut paused = HashMap::new();
        let current = config();
        // 模拟监听意外正常退出，下一次核对不能被永久 started 阻止。
        reconcile(&mut tasks, vec![current.clone()], &paused, |_, _| {
            tokio::spawn(async {})
        })
        .await;
        while !tasks["a"].handle.as_ref().unwrap().is_finished() {
            tokio::task::yield_now().await;
        }
        let factory = |_: AccountMailTransport, control: WatcherControl| {
            tokio::spawn(async move { control.cancelled().await })
        };
        reconcile(&mut tasks, vec![current.clone()], &paused, factory).await;
        assert!(!tasks["a"].control.is_stopped());
        paused.insert("a".into(), "clear-ticket".into());
        reconcile(&mut tasks, vec![current.clone()], &paused, factory).await;
        assert!(tasks.is_empty());
        paused.clear();
        reconcile(&mut tasks, vec![current], &paused, factory).await;
        assert_eq!(tasks.len(), 1);
        reconcile(&mut tasks, vec![], &paused, factory).await;
    }

    #[tokio::test]
    async fn cancelled_pause_request_waits_for_exit_then_resumes_and_signout_prevents_restart() {
        let (sender, receiver) = mpsc::unbounded_channel();
        let watchers = MailWatchers { sender };
        let desired = Arc::new(Mutex::new(vec![config()]));
        let query = desired.clone();
        let (started, mut started_rx) = mpsc::unbounded_channel();
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let released = release.clone();
        let manager = tokio::spawn(manage(
            receiver,
            move || {
                let snapshot = query.lock().unwrap().clone();
                async move { Ok(snapshot) }
            },
            move |_, control| {
                started.send(control.clone()).unwrap();
                let release = released.clone();
                tokio::spawn(async move {
                    control.cancelled().await;
                    release.acquire_owned().await.unwrap().forget();
                })
            },
        ));
        let old = started_rx.recv().await.unwrap();
        let requester = watchers.clone();
        let pause = tokio::spawn(async move { requester.pause(&AccountId::new("a")).await });
        tokio::time::timeout(Duration::from_secs(1), old.cancelled())
            .await
            .unwrap();
        assert!(!pause.is_finished());
        pause.abort();
        let _ = pause.await; // Drop 将 Resume 排在对应 Pause 后。
        assert!(
            started_rx.try_recv().is_err(),
            "旧线程未退出时不能启动新监听"
        );
        release.add_permits(1);
        let current = tokio::time::timeout(Duration::from_secs(1), started_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(!current.is_stopped());
        release.add_permits(1);
        let paused = watchers.pause(&AccountId::new("a")).await.unwrap();
        assert!(current.is_stopped());
        desired.lock().unwrap().clear(); // 暂停期间退出账号或切换到非 IMAP。
        drop(paused);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), started_rx.recv())
                .await
                .is_err()
        );
        drop(watchers);
        tokio::time::timeout(Duration::from_secs(1), manager)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn account_discovery_excludes_signed_out_accounts_and_does_not_invent_transport() {
        let db = crate::infrastructure::db::open_memory().await.unwrap();
        sqlx::query("INSERT INTO accounts(id,email,is_signed_in,created_at,updated_at) VALUES ('a','a@example.com',1,1,1),('b','b@example.com',0,1,1),('c','c@example.com',1,1,1)").execute(&db).await.unwrap();
        let mut conn = db.acquire().await.unwrap();
        mail_transports::upsert(&mut conn, &config()).await.unwrap();
        let mut signed_out = config();
        signed_out.account_id = AccountId::new("b");
        mail_transports::upsert(&mut conn, &signed_out)
            .await
            .unwrap();
        drop(conn);
        let configs = active_configs(&db).await.unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].account_id, AccountId::new("a"));
    }
}
