//! SQLite send_tasks 模块函数；连接与事务由调用方持有。

use crate::domain::entities::{SendState, SendTask};
use crate::domain::value_objects::{AccountId, MessageId, TimestampMs};
use crate::infrastructure::error::InfraError;
use sqlx::{Row, SqliteConnection};

fn map_task(row: &sqlx::sqlite::SqliteRow) -> Result<SendTask, InfraError> {
    let state_raw: String = row.try_get("state")?;
    let state = SendState::parse(&state_raw).ok_or_else(|| InfraError::Storage {
        message: format!("无效发送状态: {state_raw}"),
    })?;
    Ok(SendTask {
        task_id: row.try_get("task_id")?,
        account_id: AccountId::new(row.try_get::<String, _>("account_id")?),
        message_id: MessageId::new(row.try_get::<String, _>("message_id")?),
        content_version: row.try_get("content_version")?,
        snapshot_id: row.try_get("snapshot_id")?,
        state,
        attempt_id: row.try_get("attempt_id")?,
        claim_token: row.try_get("claim_token")?,
        lease_until: row.try_get("lease_until")?,
        attempts: row.try_get("attempts")?,
        next_attempt_at: row.try_get("next_attempt_at")?,
        last_error_code: row.try_get("last_error_code")?,
        last_error: row.try_get("last_error")?,
        accepted_at: row.try_get("accepted_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

/// 入队：与消息/快照同事务提交。
pub async fn enqueue(
    conn: &mut SqliteConnection,
    task: &SendTask,
) -> Result<(), InfraError> {
    sqlx::query(
        "INSERT INTO send_tasks
         (task_id, account_id, message_id, content_version, snapshot_id, state, attempts,
          next_attempt_at, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(account_id, task_id) DO NOTHING",
    )
    .bind(&task.task_id)
    .bind(task.account_id.as_str())
    .bind(task.message_id.as_str())
    .bind(task.content_version)
    .bind(&task.snapshot_id)
    .bind(task.state.as_str())
    .bind(task.attempts)
    .bind(task.next_attempt_at)
    .bind(task.created_at)
    .bind(task.updated_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// 认领到期任务：写入 attempt_id 与租约后才允许调用网络。
pub async fn claim_due(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    now: TimestampMs,
    lease_until: TimestampMs,
    claim_token: &str,
    attempt_id: &str,
    limit: u32,
) -> Result<Vec<SendTask>, InfraError> {
    let rows = sqlx::query(
        "SELECT * FROM send_tasks
         WHERE account_id = ?
           AND state IN ('queued', 'retryable')
           AND (next_attempt_at IS NULL OR next_attempt_at <= ?)
         ORDER BY created_at
         LIMIT ?",
    )
    .bind(account_id.as_str())
    .bind(now)
    .bind(i64::from(limit))
    .fetch_all(&mut *conn)
    .await?;

    let mut claims = Vec::new();
    for row in rows {
        let task = map_task(&row)?;
        let updated = sqlx::query(
            "UPDATE send_tasks
             SET state = 'sending', attempt_id = ?, claim_token = ?, lease_until = ?,
                 attempts = attempts + 1, updated_at = ?
             WHERE account_id = ? AND task_id = ? AND content_version = ?
               AND state IN ('queued', 'retryable')",
        )
        .bind(attempt_id)
        .bind(claim_token)
        .bind(lease_until)
        .bind(now)
        .bind(account_id.as_str())
        .bind(&task.task_id)
        .bind(task.content_version)
        .execute(&mut *conn)
        .await?;
        if updated.rows_affected() == 1 {
            let mut claimed = task;
            claimed.state = SendState::Sending;
            claimed.attempt_id = Some(attempt_id.to_string());
            claimed.claim_token = Some(claim_token.to_string());
            claimed.lease_until = Some(lease_until);
            claimed.attempts += 1;
            claimed.updated_at = now;
            claims.push(claimed);
        }
    }
    Ok(claims)
}

/// 记录一次尝试的结果；条件匹配 task/content_version/attempt_id/claim_token。
pub async fn record_result(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    task_id: &str,
    content_version: i64,
    attempt_id: &str,
    claim_token: &str,
    state: SendState,
    accepted_at: Option<TimestampMs>,
    next_attempt_at: Option<TimestampMs>,
    error_code: Option<&str>,
    error: Option<&str>,
    now: TimestampMs,
) -> Result<bool, InfraError> {
    let result = sqlx::query(
        "UPDATE send_tasks
         SET state = ?, accepted_at = COALESCE(?, accepted_at),
             next_attempt_at = ?, last_error_code = ?, last_error = ?,
             claim_token = NULL, lease_until = NULL, updated_at = ?
         WHERE account_id = ? AND task_id = ? AND content_version = ?
           AND attempt_id = ? AND claim_token = ?",
    )
    .bind(state.as_str())
    .bind(accepted_at)
    .bind(next_attempt_at)
    .bind(error_code)
    .bind(error)
    .bind(now)
    .bind(account_id.as_str())
    .bind(task_id)
    .bind(content_version)
    .bind(attempt_id)
    .bind(claim_token)
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// 启动恢复：残留 sending 按接受证据决定 accepted / uncertain，绝不自动重发。
pub async fn recover_interrupted(
    conn: &mut SqliteConnection,
    now: TimestampMs,
) -> Result<u32, InfraError> {
    let accepted = sqlx::query(
        "UPDATE send_tasks
         SET state = 'accepted', claim_token = NULL, lease_until = NULL, updated_at = ?
         WHERE state = 'sending' AND accepted_at IS NOT NULL",
    )
    .bind(now)
    .execute(&mut *conn)
    .await?
    .rows_affected();

    let uncertain = sqlx::query(
        "UPDATE send_tasks
         SET state = 'uncertain', claim_token = NULL, lease_until = NULL,
             last_error_code = COALESCE(last_error_code, 'interrupted'),
             last_error = COALESCE(last_error, '进程中断，无法确认服务器是否已接受'),
             updated_at = ?
         WHERE state = 'sending'",
    )
    .bind(now)
    .execute(&mut *conn)
    .await?
    .rows_affected();

    Ok((accepted + uncertain) as u32)
}

/// 账号下可展示的发送任务（按消息）
pub async fn list_for_account(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
) -> Result<Vec<SendTask>, InfraError> {
    let rows = sqlx::query("SELECT * FROM send_tasks WHERE account_id = ? ORDER BY created_at")
        .bind(account_id.as_str())
        .fetch_all(&mut *conn)
        .await?;
    rows.iter().map(map_task).collect()
}

pub async fn get_for_message(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Option<SendTask>, InfraError> {
    let row = sqlx::query(
        "SELECT * FROM send_tasks WHERE account_id = ? AND message_id = ? ORDER BY created_at DESC LIMIT 1",
    )
    .bind(account_id.as_str())
    .bind(message_id.as_str())
    .fetch_optional(&mut *conn)
    .await?;
    row.as_ref().map(map_task).transpose()
}
