//! IMAP IDLE / 轮询回退调度：按账号协议启动，变更时触发 sync_use_case。

use crate::application::dto::TriggerMailSyncCommand;
use crate::application::use_cases::sync_use_case;
use crate::domain::value_objects::{AccountId, MailTlsMode, ReceiveProtocol};
use crate::infrastructure::imap::{
    IdleOutcome, ImapAuth, ImapEndpoint, run_idle_until_cancel, session_login_blocking,
};
use crate::infrastructure::mail::credentials;
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

/// 为已登录且 receive=imap 的账号启动后台监视（进程级）。
pub fn spawn_watchers(
    db: SqlitePool,
    paths: AppPaths,
    changes: ChangeDispatcher,
    gate: MailSyncGate,
) {
    tauri::async_runtime::spawn(async move {
        let mut started = std::collections::HashSet::<String>::new();
        loop {
            if let Ok(mut conn) = db.acquire().await {
                if let Ok(list) = accounts::list(&mut conn).await {
                    for account in list {
                        if started.contains(account.id.as_str()) {
                            continue;
                        }
                        if let Ok(Some(t)) = mail_transports::get(&mut conn, &account.id).await {
                            if t.receive_protocol == ReceiveProtocol::Imap {
                                started.insert(account.id.as_str().to_string());
                                let db2 = db.clone();
                                let paths2 = paths.clone();
                                let changes2 = changes.clone();
                                let gate2 = gate.clone();
                                let id = account.id.clone();
                                tauri::async_runtime::spawn(async move {
                                    watch_one_account(db2, paths2, changes2, gate2, id).await;
                                });
                            }
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
}

async fn watch_one_account(
    db: SqlitePool,
    paths: AppPaths,
    changes: ChangeDispatcher,
    gate: MailSyncGate,
    account_id: AccountId,
) {
    eprintln!("IMAP 监听 [{account_id}]: 后台任务已启动，请求首次同步");
    loop {
        let ok = run_cycle(&db, &paths, &changes, &gate, &account_id).await;
        if !ok {
            eprintln!(
                "IMAP 监听 [{account_id}]: 本轮失败，{} 秒后重试",
                RECONNECT_DELAY.as_secs()
            );
            tokio::time::sleep(RECONNECT_DELAY).await;
        }
    }
}

async fn run_cycle(
    db: &SqlitePool,
    paths: &AppPaths,
    changes: &ChangeDispatcher,
    gate: &MailSyncGate,
    account_id: &AccountId,
) -> bool {
    let (transport, secret) = {
        let mut conn = match db.acquire().await {
            Ok(c) => c,
            Err(_) => return false,
        };
        let t = match mail_transports::get(&mut conn, account_id).await {
            Ok(Some(t)) => t,
            _ => return false,
        };
        if t.receive_protocol != ReceiveProtocol::Imap {
            return false;
        }
        let secret = match credentials::load(&paths.credentials_dir, account_id).await {
            Ok(Some(s)) => s,
            _ => return false,
        };
        (t, secret)
    };

    let host = match transport.imap_host.clone() {
        Some(h) => h,
        None => return false,
    };
    let port = match transport.imap_port {
        Some(p) => p,
        None => return false,
    };
    let tls = transport.imap_tls.unwrap_or(MailTlsMode::Implicit);
    let username = transport
        .username
        .clone()
        .unwrap_or_else(|| secret.username.clone());
    let auth = match (
        transport.auth_kind,
        secret.password.clone(),
        secret.access_token.clone(),
    ) {
        (crate::domain::value_objects::MailAuthKind::Password, Some(password), _) => {
            ImapAuth::Password { username, password }
        }
        (crate::domain::value_objects::MailAuthKind::OAuth2, _, Some(access_token)) => {
            ImapAuth::OAuth2 {
                username,
                access_token,
            }
        }
        _ => return false,
    };

    let endpoint = ImapEndpoint { host, port, tls };
    let use_idle = transport.imap_idle_supported.unwrap_or(false);

    // 先完成一批并提交；有历史缺口时主动续跑，不能进入无限 IDLE 等新邮件。
    if let Err(error) = sync_use_case::trigger_mail_sync(
        db,
        paths,
        changes,
        gate,
        TriggerMailSyncCommand {
            account_id: account_id.clone(),
            folder_ids: None,
        },
    )
    .await
    {
        eprintln!("IMAP 同步失败，稍后重试: {}", error.message);
        return false;
    }
    match sync_use_case::has_pending_imap_history(db, account_id).await {
        Ok(true) => {
            eprintln!("IMAP 监听 [{account_id}]: 历史尚未补齐，2 秒后请求继续同步");
            tokio::time::sleep(BACKFILL_DELAY).await;
            return true;
        }
        Err(_) => return false,
        Ok(false) => {}
    }

    if use_idle {
        eprintln!("IMAP IDLE [{account_id}]: 正在建立 INBOX 监听连接");
        let endpoint2 = endpoint.clone();
        let auth2 = auth.clone();
        let idle_result = tokio::task::spawn_blocking(move || {
            let mut session = session_login_blocking(&endpoint2, &auth2)?;
            session
                .select("INBOX")
                .map_err(|e| crate::application::error::PortError::Remote {
                    message: format!("SELECT INBOX 失败: {e}"),
                    retryable: true,
                })?;
            run_idle_until_cancel(&mut session, IDLE_TIMEOUT)
        })
        .await;
        match idle_result {
            Ok(Ok(IdleOutcome::Changed)) => {
                eprintln!("IMAP IDLE [{account_id}]: 收到服务器变化通知，请求同步");
                true
            }
            Ok(Ok(IdleOutcome::TimedOut)) => {
                eprintln!("IMAP IDLE [{account_id}]: 等待超时，请求定时兜底同步");
                true
            }
            Ok(Ok(IdleOutcome::Disconnected)) => {
                eprintln!("IMAP IDLE [{account_id}]: 监听连接已断开");
                false
            }
            Ok(Err(error)) => {
                eprintln!("IMAP IDLE [{account_id}]: 监听建立失败: {error}");
                false
            }
            Err(error) => {
                eprintln!("IMAP IDLE [{account_id}]: 监听任务异常: {error}");
                false
            }
        }
    } else {
        eprintln!(
            "IMAP 监听 [{account_id}]: IDLE 不可用或尚未探测，{} 秒后请求轮询同步",
            POLL_INTERVAL.as_secs()
        );
        tokio::time::sleep(POLL_INTERVAL).await;
        true
    }
}
