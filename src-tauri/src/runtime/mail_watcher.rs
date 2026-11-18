//! IMAP IDLE / 轮询回退调度：按账号协议启动，变更时触发 sync_use_case。

use crate::application::dto::TriggerMailSyncCommand;
use crate::application::use_cases::sync_use_case;
use crate::domain::entities::ReceiveConfig;
use crate::domain::value_objects::AccountId;
use crate::infrastructure::imap::{IdleOutcome, load_connection, run_idle_until_cancel, session_login};
use crate::infrastructure::paths::AppPaths;
use crate::infrastructure::repository::{accounts, mail_transports};
use crate::runtime::{ChangeDispatcher, MailSyncGate};
use sqlx::SqlitePool;
use std::time::Duration;

// 即使 INBOX 不变，也定期同步其他勾选夹及重新检查账号配置。
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
    control: super::mail_watchers::WatcherControl,
) {
    eprintln!("IMAP 监听 [{account_id}]: 后台任务已启动，请求首次同步");
    while !control.is_stopped() {
        let ok = run_cycle(&db, &paths, &changes, &gate, &account_id, &control).await;
        if control.is_stopped() {
            break;
        }
        if !ok {
            eprintln!(
                "IMAP 监听 [{account_id}]: 本轮失败，{} 秒后重试",
                RECONNECT_DELAY.as_secs()
            );
            control.sleep(RECONNECT_DELAY).await;
        }
    }
}

async fn run_cycle(
    db: &SqlitePool,
    paths: &AppPaths,
    changes: &ChangeDispatcher,
    gate: &MailSyncGate,
    account_id: &AccountId,
    control: &super::mail_watchers::WatcherControl,
) -> bool {
    let (endpoint, auth, use_idle) = {
        let mut conn = match db.acquire().await {
            Ok(c) => c,
            Err(_) => return false,
        };
        if !accounts::is_signed_in(&mut conn, account_id)
            .await
            .unwrap_or(false)
        {
            return false;
        }
        let transport = match mail_transports::get(&mut conn, account_id).await {
            Ok(Some(t)) => t,
            _ => return false,
        };
        let ReceiveConfig::Imap(config) = &transport.receive else {
            return false;
        };
        let (endpoint, auth) = match load_connection(&paths.credentials_dir, config).await {
            Ok(v) => v,
            _ => return false,
        };
        (
            endpoint,
            auth,
            transport.imap_idle_supported.unwrap_or(false),
        )
    };

    // 先完成一批并提交；有历史缺口时主动续跑，不能进入无限 IDLE 等新邮件。
    let Some(operation) = control.begin_operation(Duration::from_secs(10 * 60)) else {
        return false;
    };
    match crate::runtime::mail_sync::trigger_mail_sync_controlled(
        db,
        paths,
        changes,
        gate,
        TriggerMailSyncCommand {
            account_id: account_id.clone(),
            folder_ids: None,
        },
        operation,
    )
    .await
    {
        Ok(Some(changeset)) => changes.dispatch(changeset),
        Ok(None) => {}
        Err(error) => {
            eprintln!("IMAP 同步失败，稍后重试: {}", error.message);
            return false;
        }
    }
    if control.is_stopped() {
        return false;
    }
    match sync_use_case::has_pending_imap_history(db, account_id).await {
        Ok(true) => {
            eprintln!("IMAP 监听 [{account_id}]: 历史尚未补齐，2 秒后请求继续同步");
            control.sleep(BACKFILL_DELAY).await;
            return true;
        }
        Err(_) => return false,
        Ok(false) => {}
    }

    if use_idle {
        eprintln!("IMAP IDLE [{account_id}]: 正在建立 INBOX 监听连接");
        let Some(operation) = control.begin_operation(Duration::from_secs(150)) else {
            return false;
        };
        let mut session = match session_login(&endpoint, &auth, &operation).await {
            Ok(session) => session,
            Err(error) => {
                eprintln!("IMAP IDLE [{account_id}]: 登录失败: {error}");
                return false;
            }
        };
        let idle_result = operation
            .run_blocking(move || {
                session.select("INBOX").map_err(|e| {
                    crate::infrastructure::error::InfraError::Remote {
                        message: format!("SELECT INBOX 失败: {e}"),
                        retryable: true,
                    }
                })?;
                run_idle_until_cancel(&mut session, IDLE_TIMEOUT)
            })
            .await;
        match idle_result {
            Ok(IdleOutcome::Changed) => {
                eprintln!("IMAP IDLE [{account_id}]: 收到服务器变化通知，请求同步");
                true
            }
            Ok(IdleOutcome::TimedOut) => {
                eprintln!("IMAP IDLE [{account_id}]: 等待超时，请求定时兜底同步");
                true
            }
            Ok(IdleOutcome::Disconnected) => {
                eprintln!("IMAP IDLE [{account_id}]: 监听连接已断开");
                false
            }
            Err(error) => {
                eprintln!("IMAP IDLE [{account_id}]: 监听失败: {error}");
                false
            }
        }
    } else {
        eprintln!(
            "IMAP 监听 [{account_id}]: IDLE 不可用或尚未探测，{} 秒后请求轮询同步",
            POLL_INTERVAL.as_secs()
        );
        control.sleep(POLL_INTERVAL).await;
        true
    }
}
