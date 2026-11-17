//! Graph 账户定时同步与 mutation 推送（不含聚合 drain）。

use crate::application::dto::TriggerMailSyncCommand;
use crate::application::use_cases::message_use_case;
use crate::domain::value_objects::ReceiveProtocol;
use crate::infrastructure::paths::AppPaths;
use crate::infrastructure::repository::{accounts, mail_transports};
use crate::runtime::{ChangeDispatcher, MailSyncGate};
use sqlx::SqlitePool;
use std::time::Duration;

const TICK: Duration = Duration::from_secs(120);

/// 启动固定间隔调度；账户级不重入由 `MailSyncGate` 保证。
pub fn spawn(db: SqlitePool, paths: AppPaths, changes: ChangeDispatcher, gate: MailSyncGate) {
    tauri::async_runtime::spawn(async move {
        loop {
            tick_once(&db, &paths, &changes, &gate).await;
            tokio::time::sleep(TICK).await;
        }
    });
}

async fn tick_once(
    db: &SqlitePool,
    paths: &AppPaths,
    changes: &ChangeDispatcher,
    gate: &MailSyncGate,
) {
    let list = {
        let mut conn = match db.acquire().await {
            Ok(c) => c,
            Err(_) => return,
        };
        match accounts::list(&mut conn).await {
            Ok(v) => v,
            Err(_) => return,
        }
    };

    for account in list {
        if !account_is_active_for_graph_sync(db, &account.id).await {
            continue;
        }

        let db2 = db.clone();
        let paths2 = paths.clone();
        let changes2 = changes.clone();
        let gate2 = gate.clone();
        let account_id = account.id.clone();
        tauri::async_runtime::spawn(async move {
            let _ = crate::runtime::mail_sync::trigger_mail_sync(
                &db2,
                &paths2,
                &changes2,
                &gate2,
                TriggerMailSyncCommand {
                    account_id: account_id.clone(),
                    folder_ids: None,
                },
            )
            .await;
            let _ = message_use_case::flush_due_mutations(&db2, &paths2, &account_id, 32).await;
        });
    }
}

async fn account_is_active_for_graph_sync(
    db: &SqlitePool,
    account_id: &crate::domain::value_objects::AccountId,
) -> bool {
    let mut conn = match db.acquire().await {
        Ok(c) => c,
        Err(_) => return false,
    };
    let Ok(Some(t)) = mail_transports::get(&mut conn, account_id).await else {
        return false;
    };
    t.receive.protocol() == ReceiveProtocol::Graph
}
