//! 发送队列 worker：通过账号受控入口认领并执行一次尝试。
//!
//! 每次尝试先持久化 attempt_id 与租约，再调用网络；结果按阶段分类落库。
//! 结果不确定（尤其 DATA 之后）不自动重发。

use crate::application::error::AppError;
use crate::application::use_cases::outbound_use_case;
use crate::domain::entities::{backoff_ms, next_state, SendState};
use crate::domain::value_objects::AccountId;
use crate::infrastructure::mail::operation::MailOperation;
use crate::infrastructure::paths::AppPaths;
use crate::infrastructure::repository::{accounts, messages, send_tasks};
use crate::runtime::{ChangeDispatcher, MailSyncGate};
use sqlx::SqlitePool;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const TICK: Duration = Duration::from_secs(5);
const LEASE: Duration = Duration::from_secs(5 * 60);

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 入队后立即尝试一次派发（不阻塞调用方）。
pub fn kick(db: SqlitePool, paths: AppPaths, changes: ChangeDispatcher, gate: MailSyncGate) {
    tauri::async_runtime::spawn(async move {
        let _ = tick_once(&db, &paths, &changes, &gate).await;
    });
}

/// 启动周期派发；启动时先把残留 sending 收尾为 accepted/uncertain。
pub fn spawn(
    db: SqlitePool,
    paths: AppPaths,
    changes: ChangeDispatcher,
    gate: MailSyncGate,
) {
    tauri::async_runtime::spawn(async move {
        recover_interrupted(&db, &changes).await;
        loop {
            if let Err(error) = tick_once(&db, &paths, &changes, &gate).await {
                eprintln!("发送队列派发失败: {}", error.message);
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

async fn recover_interrupted(db: &SqlitePool, changes: &ChangeDispatcher) {
    let Ok(mut conn) = db.acquire().await else {
        return;
    };
    let Ok(affected) = send_tasks::recover_interrupted(&mut conn, now_ms()).await else {
        return;
    };
    for (account_id, message_id) in affected {
        let Ok(version) = accounts::bump_change_version(&mut conn, &account_id).await else {
            continue;
        };
        dispatch_send_change(db, changes, &account_id, &message_id, version).await;
    }
}

/// 发送状态变化的统一通知：持久版本与真实 ChangeSet 一起发出。
async fn dispatch_send_change(
    db: &SqlitePool,
    changes: &ChangeDispatcher,
    account_id: &AccountId,
    message_id: &crate::domain::value_objects::MessageId,
    version: i64,
) {
    let mut set = crate::application::dto::ChangeSet::empty(
        account_id.clone(),
        crate::application::dto::ChangeOrigin::UserAction,
    );
    set.message_ids.push(message_id.clone());
    set.account_version = Some(version);
    if let Ok(mut conn) = db.acquire().await {
        if let Ok(session_ids) =
            messages::list_session_ids(&mut conn, account_id, std::slice::from_ref(message_id)).await
        {
            set.session_ids = session_ids;
        }
    }
    changes.dispatch(set);
}

async fn tick_once(
    db: &SqlitePool,
    paths: &AppPaths,
    changes: &ChangeDispatcher,
    gate: &MailSyncGate,
) -> Result<(), AppError> {
    let account_ids = {
        let mut conn = db.acquire().await?;
        accounts::list(&mut conn)
            .await
            .map_err(AppError::from)?
            .into_iter()
            .map(|a| a.id)
            .collect::<Vec<_>>()
    };

    for account_id in account_ids {
        // 先取得账号许可再认领：许可覆盖网络调用与结果落库整个尝试。
        let operation = MailOperation::new(Duration::from_secs(120));
        let Some(permit) =
            gate.try_acquire_operation(account_id.as_str(), Some(operation.clone()))
        else {
            continue;
        };
        let (claimed, claimed_version) = {
            let mut tx = db.begin().await?;
            let attempt_id = uuid::Uuid::new_v4().to_string();
            let claim_token = uuid::Uuid::new_v4().to_string();
            let now = now_ms();
            let claimed = send_tasks::claim_due(
                &mut tx,
                &account_id,
                now,
                now + LEASE.as_millis() as i64,
                &claim_token,
                &attempt_id,
                1,
            )
            .await
            .map_err(AppError::from)?;
            let version = if claimed.is_empty() {
                None
            } else {
                Some(
                    accounts::bump_change_version(&mut tx, &account_id)
                        .await
                        .map_err(AppError::from)?,
                )
            };
            tx.commit().await?;
            (claimed, version)
        };
        if claimed.is_empty() {
            continue;
        }
        if let Some(version) = claimed_version {
            for task in &claimed {
                dispatch_send_change(db, changes, &account_id, &task.message_id, version).await;
            }
        }
        let db2 = db.clone();
        let paths2 = paths.clone();
        let changes2 = changes.clone();
        let account2 = account_id.clone();
        let operation2 = operation.clone();
        tauri::async_runtime::spawn(async move {
            let _permit = permit;
            for task in claimed {
                if let Err(error) =
                    run_attempt(&db2, &paths2, &changes2, &operation2, &account2, &task).await
                {
                    eprintln!("发送尝试失败: {}", error.message);
                }
            }
        });
    }
    Ok(())
}

/// 单次尝试：必须持有账号 permit；退出/换配置必须等待本次尝试收尾。
async fn run_attempt(
    db: &SqlitePool,
    paths: &AppPaths,
    changes: &ChangeDispatcher,
    operation: &MailOperation,
    account_id: &AccountId,
    task: &crate::domain::entities::SendTask,
) -> Result<(), AppError> {
    let now = now_ms();
    if operation.checkpoint().is_err() {
        // 取消发生在远端写入前：退回可重试，不触碰网络；本地失败只重试该收尾。
        let mut tx = db.begin().await?;
        let applied = send_tasks::record_result(
            &mut tx,
            account_id,
            &task.task_id,
            task.content_version,
            task.attempt_id.as_deref().unwrap_or_default(),
            task.claim_token.as_deref().unwrap_or_default(),
            SendState::Retryable,
            None,
            Some(now + backoff_ms(task.attempts)),
            Some("cancelled"),
            Some("账号任务已取消，尚未开始发送"),
            now,
        )
        .await
        .map_err(AppError::from)?;
        let version = if applied {
            Some(
                accounts::bump_change_version(&mut tx, account_id)
                    .await
                    .map_err(AppError::from)?,
            )
        } else {
            None
        };
        tx.commit().await?;
        if let Some(version) = version {
            dispatch_send_change(db, changes, account_id, &task.message_id, version).await;
        }
        return Ok(());
    }

    let result =
        outbound_use_case::execute_claimed_attempt(db, paths, operation, account_id, task).await;

    let now = now_ms();
    match result {
        Ok(outcome) => {
            let next = next_state(outcome.outcome, outcome.stage);
            if next == SendState::Accepted {
                // 只重试本地确认步骤，绝不因本地失败重新发送邮件。
                let mut last_error = None;
                for _ in 0..3 {
                    match outbound_use_case::complete_accepted_attempt(db, account_id, task, now)
                        .await
                    {
                        Ok(Some(version)) => {
                            dispatch_send_change(db, changes, account_id, &task.message_id, version)
                                .await;
                            return Ok(());
                        }
                        Ok(None) => return Ok(()),
                        Err(error) => {
                            last_error = Some(error);
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                    }
                }
                return Err(last_error
                    .unwrap_or_else(|| AppError::internal("发送确认落库失败")));
            }
            let next_attempt = (next == SendState::Retryable)
                .then(|| now + backoff_ms(task.attempts));
            let mut tx = db.begin().await?;
            let applied = send_tasks::record_result(
                &mut tx,
                account_id,
                &task.task_id,
                task.content_version,
                task.attempt_id.as_deref().unwrap_or_default(),
                task.claim_token.as_deref().unwrap_or_default(),
                next,
                None,
                next_attempt,
                None,
                None,
                now,
            )
            .await
            .map_err(AppError::from)?;
            let version = if applied {
                Some(
                    accounts::bump_change_version(&mut tx, account_id)
                        .await
                        .map_err(AppError::from)?,
                )
            } else {
                None
            };
            tx.commit().await?;
            if let Some(version) = version {
                dispatch_send_change(db, changes, account_id, &task.message_id, version).await;
            }
            Ok(())
        }
        Err(error) => {
            // 错误的阶段不可证明：认证/永久错误确定未接受；可重试错误仅来自
            // 连接阶段或明确 4xx，按 Retryable 处理。
            let next = if error.code == crate::application::error::AppErrorCode::Auth {
                SendState::Failed
            } else if error.retryable {
                SendState::Retryable
            } else {
                SendState::Failed
            };
            let next_attempt = (next == SendState::Retryable)
                .then(|| now + backoff_ms(task.attempts));
            let mut tx = db.begin().await?;
            let applied = send_tasks::record_result(
                &mut tx,
                account_id,
                &task.task_id,
                task.content_version,
                task.attempt_id.as_deref().unwrap_or_default(),
                task.claim_token.as_deref().unwrap_or_default(),
                next,
                None,
                next_attempt,
                Some("send_failed"),
                Some(&error.message),
                now,
            )
            .await
            .map_err(AppError::from)?;
            let version = if applied {
                Some(
                    accounts::bump_change_version(&mut tx, account_id)
                        .await
                        .map_err(AppError::from)?,
                )
            } else {
                None
            };
            tx.commit().await?;
            if let Some(version) = version {
                dispatch_send_change(db, changes, account_id, &task.message_id, version).await;
            }
            Ok(())
        }
    }
}
