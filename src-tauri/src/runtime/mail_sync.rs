//! 账号任务持有 gate；取消等待者只发信号，后台任务必须完成退出与收尾。

use crate::application::dto::TriggerMailSyncCommand;
use crate::application::error::AppError;
use crate::application::use_cases::sync_use_case;
use crate::domain::entities::MailFolderSyncState;
use crate::domain::value_objects::AccountId;
use crate::infrastructure::mail::operation::MailOperation;
use crate::infrastructure::paths::AppPaths;
use crate::runtime::{ChangeDispatcher, MailSyncGate};
use sqlx::SqlitePool;
use std::future::Future;
use std::time::Duration;

async fn run_owned<T, F, Fut>(
    gate: &MailSyncGate,
    account_id: &str,
    operation: MailOperation,
    work: F,
) -> Result<Option<T>, AppError>
where
    T: Send + 'static,
    F: FnOnce(MailOperation) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, AppError>> + Send + 'static,
{
    let Some(permit) = gate.try_acquire_operation(account_id, Some(operation.clone())) else {
        return Ok(None);
    };
    let guard = operation.cancel_on_drop();
    let worker = tokio::spawn(async move {
        let _permit = permit;
        work(operation).await
    });
    let result = worker
        .await
        .map_err(|_| AppError::internal("邮件任务异常退出"))?;
    guard.disarm();
    result.map(Some)
}

pub async fn trigger_mail_sync(
    db: &SqlitePool,
    paths: &AppPaths,
    changes: &ChangeDispatcher,
    gate: &MailSyncGate,
    cmd: TriggerMailSyncCommand,
) -> Result<(), AppError> {
    trigger_mail_sync_controlled(
        db,
        paths,
        changes,
        gate,
        cmd,
        MailOperation::new(Duration::from_secs(10 * 60)),
    )
    .await
}

pub(crate) async fn trigger_mail_sync_controlled(
    db: &SqlitePool,
    paths: &AppPaths,
    changes: &ChangeDispatcher,
    gate: &MailSyncGate,
    cmd: TriggerMailSyncCommand,
    operation: MailOperation,
) -> Result<(), AppError> {
    let account_id = cmd.account_id.to_string();
    let db = db.clone();
    let paths = paths.clone();
    let changes = changes.clone();
    run_owned(gate, &account_id, operation, move |operation| async move {
        sync_use_case::trigger_mail_sync(&db, &paths, &changes, &operation, cmd).await
    })
    .await?;
    Ok(())
}

pub async fn refresh_mail_folders(
    db: &SqlitePool,
    paths: &AppPaths,
    gate: &MailSyncGate,
    account_id: &AccountId,
) -> Result<Vec<MailFolderSyncState>, AppError> {
    let db = db.clone();
    let paths = paths.clone();
    let account = account_id.clone();
    run_owned(
        gate,
        account_id.as_str(),
        MailOperation::new(Duration::from_secs(90)),
        move |operation| async move {
            sync_use_case::refresh_mail_folders(&db, &paths, &operation, &account).await
        },
    )
    .await?
    .ok_or_else(|| AppError::conflict("邮件同步进行中，请稍后刷新文件夹"))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn wait_for_release(gate: &MailSyncGate) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if gate.try_acquire("a").is_some() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn cancelled_caller_does_not_release_gate_while_worker_is_draining() {
        let gate = MailSyncGate::new();
        let gate2 = gate.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let (control_tx, control_rx) = tokio::sync::oneshot::channel();
        let caller = tokio::spawn(async move {
            run_owned(
                &gate2,
                "a",
                MailOperation::new(Duration::from_secs(5)),
                move |operation| async move {
                    control_tx.send(operation.clone()).ok();
                    operation
                        .run_blocking(move || {
                            started_tx.send(()).unwrap();
                            // 模拟关闭 socket 后尚未结束的线程收尾。
                            release_rx.recv().unwrap();
                            Ok(())
                        })
                        .await
                        .map_err(AppError::from)
                },
            )
            .await
        });
        let operation = control_rx.await.unwrap();
        started_rx.await.unwrap();
        caller.abort();
        let _ = caller.await;
        tokio::time::timeout(Duration::from_secs(1), operation.cancelled())
            .await
            .unwrap();
        assert!(gate.try_acquire("a").is_none());
        assert!(gate.try_acquire("b").is_some());
        release_tx.send(()).unwrap();
        wait_for_release(&gate).await;
    }

    #[tokio::test]
    async fn timeout_drains_worker_before_returning_or_allowing_the_next_sync() {
        let gate = MailSyncGate::new();
        let gate2 = gate.clone();
        let (control_tx, control_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let caller = tokio::spawn(async move {
            run_owned(
                &gate2,
                "a",
                MailOperation::new(Duration::from_millis(100)),
                move |operation| async move {
                    control_tx.send(operation.clone()).ok();
                    operation
                        .run_blocking(move || {
                            started_tx.send(()).unwrap();
                            release_rx.recv().unwrap();
                            Ok(())
                        })
                        .await
                        .map_err(AppError::from)
                },
            )
            .await
        });
        let operation = control_rx.await.unwrap();
        started_rx.await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), operation.cancelled())
            .await
            .unwrap();
        assert!(!caller.is_finished());
        assert!(gate.try_acquire("a").is_none());
        release_tx.send(()).unwrap();
        assert!(caller.await.unwrap().is_err());
        assert!(gate.try_acquire("a").is_some());
    }
}
