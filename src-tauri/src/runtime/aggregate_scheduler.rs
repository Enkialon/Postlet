//! 聚合任务 drain 调度（固定间隔）。

use crate::application::use_cases::aggregate_worker_use_case;
use crate::runtime::ChangeDispatcher;
use sqlx::SqlitePool;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const TICK: Duration = Duration::from_secs(5);

/// 启动聚合 drain；全局不重入。
pub fn spawn(db: SqlitePool, changes: ChangeDispatcher) {
    let running = Arc::new(AtomicBool::new(false));
    tauri::async_runtime::spawn(async move {
        loop {
            if !running.swap(true, Ordering::SeqCst) {
                let db2 = db.clone();
                let changes2 = changes.clone();
                let running2 = running.clone();
                tauri::async_runtime::spawn(async move {
                    let _ =
                        aggregate_worker_use_case::process_available_jobs(&db2, Some(&changes2), 32)
                            .await;
                    running2.store(false, Ordering::SeqCst);
                });
            }
            tokio::time::sleep(TICK).await;
        }
    });
}
