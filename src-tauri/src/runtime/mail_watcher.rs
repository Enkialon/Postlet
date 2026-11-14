//! IMAP IDLE / 轮询回退调度：按账号协议启动，变更时触发 sync_use_case。

use crate::application::dto::TriggerMailSyncCommand;
use crate::application::use_cases::sync_use_case;
use crate::domain::value_objects::{AccountId, MailTlsMode, ReceiveProtocol};
use crate::infrastructure::imap::{
    run_idle_until_cancel, session_login_blocking, IdleOutcome, ImapAuth, ImapEndpoint,
};
use crate::infrastructure::mail::credentials;
use crate::infrastructure::paths::AppPaths;
use crate::infrastructure::repository::{accounts, mail_transports};
use crate::runtime::ChangeDispatcher;
use sqlx::SqlitePool;
use std::time::Duration;

const IDLE_TIMEOUT: Duration = Duration::from_secs(25 * 60);
const POLL_INTERVAL: Duration = Duration::from_secs(60);
const RECONNECT_DELAY: Duration = Duration::from_secs(15);

/// 为已登录且 receive=imap 的账号启动后台监视（进程级）。
pub fn spawn_watchers(db: SqlitePool, paths: AppPaths, changes: ChangeDispatcher) {
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
                                let id = account.id.clone();
                                tauri::async_runtime::spawn(async move {
                                    watch_one_account(db2, paths2, changes2, id).await;
                                });
                            }
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(300)).await;
        }
    });
}

async fn watch_one_account(
    db: SqlitePool,
    paths: AppPaths,
    changes: ChangeDispatcher,
    account_id: AccountId,
) {
    loop {
        let ok = run_cycle(&db, &paths, &changes, &account_id).await;
        if !ok {
            tokio::time::sleep(RECONNECT_DELAY).await;
        }
    }
}

async fn run_cycle(
    db: &SqlitePool,
    paths: &AppPaths,
    changes: &ChangeDispatcher,
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

    if use_idle {
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
            Ok(Ok(IdleOutcome::Changed | IdleOutcome::TimedOut)) => {
                let _ = sync_use_case::trigger_mail_sync(
                    db,
                    paths,
                    changes,
                    TriggerMailSyncCommand {
                        account_id: account_id.clone(),
                        folder_ids: None,
                    },
                )
                .await;
                true
            }
            _ => false,
        }
    } else {
        tokio::time::sleep(POLL_INTERVAL).await;
        let _ = sync_use_case::trigger_mail_sync(
            db,
            paths,
            changes,
            TriggerMailSyncCommand {
                account_id: account_id.clone(),
                folder_ids: None,
            },
        )
        .await;
        true
    }
}
