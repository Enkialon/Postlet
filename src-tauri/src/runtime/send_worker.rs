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
use crate::infrastructure::repository::{accounts, send_tasks};
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
        recover_interrupted(&db).await;
        loop {
            if let Err(error) = tick_once(&db, &paths, &changes, &gate).await {
                eprintln!("发送队列派发失败: {}", error.message);
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

async fn recover_interrupted(db: &SqlitePool) {
    if let Ok(mut conn) = db.acquire().await {
        let _ = send_tasks::recover_interrupted(&mut conn, now_ms()).await;
    }
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
        let claimed = {
            let mut conn = db.acquire().await?;
            let attempt_id = uuid::Uuid::new_v4().to_string();
            let claim_token = uuid::Uuid::new_v4().to_string();
            let now = now_ms();
            send_tasks::claim_due(
                &mut conn,
                &account_id,
                now,
                now + LEASE.as_millis() as i64,
                &claim_token,
                &attempt_id,
                4,
            )
            .await
            .map_err(AppError::from)?
        };
        if claimed.is_empty() {
            continue;
        }
        if claimed.len() > 0 {
            let db2 = db.clone();
            let paths2 = paths.clone();
            let changes2 = changes.clone();
            let gate2 = gate.clone();
            let account2 = account_id.clone();
            tauri::async_runtime::spawn(async move {
                for task in claimed {
                    if let Err(error) =
                        run_attempt(&db2, &paths2, &changes2, &gate2, &account2, &task).await
                    {
                        eprintln!("发送尝试失败: {}", error.message);
                    }
                }
            });
        }
    }
    Ok(())
}

/// 单次尝试：必须持有账号 permit；退出/换配置必须等待本次尝试收尾。
async fn run_attempt(
    db: &SqlitePool,
    paths: &AppPaths,
    changes: &ChangeDispatcher,
    gate: &MailSyncGate,
    account_id: &AccountId,
    task: &crate::domain::entities::SendTask,
) -> Result<(), AppError> {
    let operation = MailOperation::new(Duration::from_secs(120));
    let Some(permit) = gate.try_acquire_operation(account_id.as_str(), Some(operation.clone()))
    else {
        // 账号忙：放回队列稍后重试，不触碰网络
        let mut conn = db.acquire().await?;
        let now = now_ms();
        send_tasks::record_result(
            &mut conn,
            account_id,
            &task.task_id,
            task.content_version,
            task.attempt_id.as_deref().unwrap_or_default(),
            task.claim_token.as_deref().unwrap_or_default(),
            SendState::Retryable,
            None,
            Some(now + backoff_ms(task.attempts)),
            Some("busy"),
            Some("账号正在执行其他邮件任务"),
            now,
        )
        .await
        .map_err(AppError::from)?;
        return Ok(());
    };

    let result = outbound_use_case::execute_claimed_attempt(db, paths, account_id, task).await;
    drop(permit);

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
                        Ok(true) => {
                            changes.dispatch(crate::application::dto::ChangeSet::empty(
                                account_id.clone(),
                                crate::application::dto::ChangeOrigin::UserAction,
                            ));
                            return Ok(());
                        }
                        Ok(false) => return Ok(()),
                        Err(error) => {
                            last_error = Some(error);
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                    }
                }
                return Err(last_error
                    .unwrap_or_else(|| AppError::internal("发送确认落库失败")));
            }
            let mut conn = db.acquire().await?;
            let next_attempt = (next == SendState::Retryable)
                .then(|| now + backoff_ms(task.attempts));
            send_tasks::record_result(
                &mut conn,
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
            let mut conn = db.acquire().await?;
            send_tasks::record_result(
                &mut conn,
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
            Ok(())
        }
    }
}
