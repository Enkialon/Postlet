//! SQLite session_aggregate_jobs；连接与事务由调用方持有。

use crate::infrastructure::error::InfraError;
use crate::domain::entities::SessionAggregateJob;
use crate::domain::value_objects::{
    AccountId, AggregateJobStatus, AggregationOutcome, MessageId, TimestampMs,
};
use sqlx::{Row, SqliteConnection};

fn map_job(row: &sqlx::sqlite::SqliteRow) -> Result<SessionAggregateJob, InfraError> {
    let status_raw: String = row.try_get("status")?;
    let status = AggregateJobStatus::parse(&status_raw).ok_or_else(|| InfraError::Storage {
        message: format!("无效 job status: {status_raw}"),
    })?;
    let outcome = row
        .try_get::<Option<String>, _>("outcome")?
        .map(|s| {
            AggregationOutcome::parse(&s).ok_or_else(|| InfraError::Storage {
                message: format!("无效 job outcome: {s}"),
            })
        })
        .transpose()?;
    Ok(SessionAggregateJob {
        account_id: AccountId::new(row.try_get::<String, _>("account_id")?),
        message_id: MessageId::new(row.try_get::<String, _>("message_id")?),
        generation: row.try_get("generation")?,
        outcome,
        status,
        attempts: row.try_get::<i64, _>("attempts")? as i32,
        last_error: row.try_get("last_error")?,
        available_at: row.try_get("available_at")?,
        updated_at: row.try_get("updated_at")?,
        lease_token: row.try_get("lease_token")?,
        lease_expires_at: row.try_get("lease_expires_at")?,
    })
}

/// 入队或重置：升 generation、清租约、回到 pending。
pub async fn enqueue_or_reset(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_id: &MessageId,
    available_at: TimestampMs,
) -> Result<(), InfraError> {
    let now = available_at;
    sqlx::query(
        "INSERT INTO session_aggregate_jobs (
            message_id, account_id, generation, outcome, status, attempts,
            last_error, available_at, updated_at, lease_token, lease_expires_at
         ) VALUES (?, ?, 1, NULL, 'pending', 0, NULL, ?, ?, NULL, NULL)
         ON CONFLICT(account_id, message_id) DO UPDATE SET
            generation = session_aggregate_jobs.generation + 1,
            outcome = NULL,
            status = 'pending',
            attempts = 0,
            last_error = NULL,
            available_at = excluded.available_at,
            updated_at = excluded.updated_at,
            lease_token = NULL,
            lease_expires_at = NULL",
    )
    .bind(message_id.as_str())
    .bind(account_id.as_str())
    .bind(available_at)
    .bind(now)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn claim_batch(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    now: TimestampMs,
    lease_token: &str,
    lease_expires_at: TimestampMs,
    limit: u32,
) -> Result<Vec<SessionAggregateJob>, InfraError> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let updated = sqlx::query(
        "UPDATE session_aggregate_jobs
         SET status = 'processing',
             lease_token = ?,
             lease_expires_at = ?,
             attempts = attempts + 1,
             updated_at = ?
         WHERE rowid IN (
            SELECT rowid FROM session_aggregate_jobs
            WHERE account_id = ?
              AND status IN ('pending', 'retry')
              AND available_at <= ?
            ORDER BY available_at ASC, generation ASC
            LIMIT ?
         )",
    )
    .bind(lease_token)
    .bind(lease_expires_at)
    .bind(now)
    .bind(account_id.as_str())
    .bind(now)
    .bind(i64::from(limit))
    .execute(&mut *conn)
    .await?
    .rows_affected();

    if updated == 0 {
        return Ok(Vec::new());
    }

    let rows = sqlx::query(
        "SELECT * FROM session_aggregate_jobs
         WHERE account_id = ? AND lease_token = ? AND status = 'processing'
         ORDER BY available_at ASC",
    )
    .bind(account_id.as_str())
    .bind(lease_token)
    .fetch_all(&mut *conn)
    .await?;
    rows.iter().map(map_job).collect()
}

pub async fn complete(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_id: &MessageId,
    lease_token: &str,
    generation: i64,
    outcome: AggregationOutcome,
) -> Result<(), InfraError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let result = sqlx::query(
        "UPDATE session_aggregate_jobs
         SET status = 'completed',
             outcome = ?,
             last_error = NULL,
             lease_token = NULL,
             lease_expires_at = NULL,
             updated_at = ?
         WHERE account_id = ?
           AND message_id = ?
           AND lease_token = ?
           AND generation = ?
           AND status = 'processing'",
    )
    .bind(outcome.as_str())
    .bind(now)
    .bind(account_id.as_str())
    .bind(message_id.as_str())
    .bind(lease_token)
    .bind(generation)
    .execute(&mut *conn)
    .await?;
    if result.rows_affected() == 0 {
        return Err(InfraError::Conflict {
            message: "聚合任务租约或代次不匹配，拒绝 complete".into(),
        });
    }
    Ok(())
}

pub async fn fail_for_retry(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_id: &MessageId,
    lease_token: &str,
    generation: i64,
    error: &str,
    available_at: TimestampMs,
) -> Result<(), InfraError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let result = sqlx::query(
        "UPDATE session_aggregate_jobs
         SET status = 'retry',
             last_error = ?,
             available_at = ?,
             lease_token = NULL,
             lease_expires_at = NULL,
             updated_at = ?
         WHERE account_id = ?
           AND message_id = ?
           AND lease_token = ?
           AND generation = ?
           AND status = 'processing'",
    )
    .bind(error)
    .bind(available_at)
    .bind(now)
    .bind(account_id.as_str())
    .bind(message_id.as_str())
    .bind(lease_token)
    .bind(generation)
    .execute(&mut *conn)
    .await?;
    if result.rows_affected() == 0 {
        return Err(InfraError::Conflict {
            message: "聚合任务租约或代次不匹配，拒绝 retry".into(),
        });
    }
    Ok(())
}

pub async fn fail_permanently(
    conn: &mut SqliteConnection,
    claim: &AggregateClaim,
    error: &str,
) -> Result<(), InfraError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let result = sqlx::query(
        "UPDATE session_aggregate_jobs
         SET status = 'failed',
             last_error = ?,
             lease_token = NULL,
             lease_expires_at = NULL,
             updated_at = ?
         WHERE account_id = ?
           AND message_id = ?
           AND lease_token = ?
           AND generation = ?
           AND status = 'processing'",
    )
    .bind(error)
    .bind(now)
    .bind(claim.account_id.as_str())
    .bind(claim.message_id.as_str())
    .bind(&claim.lease_token)
    .bind(claim.generation)
    .execute(&mut *conn)
    .await?;
    if result.rows_affected() == 0 {
        return Err(InfraError::Conflict {
            message: "聚合任务租约或代次不匹配，拒绝 fail".into(),
        });
    }
    Ok(())
}

pub async fn release_expired_leases(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    now: TimestampMs,
) -> Result<u32, InfraError> {
    let result = sqlx::query(
        "UPDATE session_aggregate_jobs
         SET status = 'retry',
             lease_token = NULL,
             lease_expires_at = NULL,
             last_error = COALESCE(last_error, 'lease expired'),
             available_at = ?,
             updated_at = ?
         WHERE account_id = ?
           AND status = 'processing'
           AND lease_expires_at IS NOT NULL
           AND lease_expires_at < ?",
    )
    .bind(now)
    .bind(now)
    .bind(account_id.as_str())
    .bind(now)
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected() as u32)
}

/// 列出账户内可重投影的邮件 id（非远端删除）。
pub async fn list_message_ids_for_reaggregate(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    limit: u32,
) -> Result<Vec<MessageId>, InfraError> {
    let rows = sqlx::query_scalar::<_, String>(
        "SELECT id FROM messages
         WHERE account_id = ? AND sync_status != 'deleted_remote' AND (location_status = 'present' OR (location_status IS NULL AND sync_status = 'pending_local'))
         ORDER BY local_updated_at DESC
         LIMIT ?",
    )
    .bind(account_id.as_str())
    .bind(i64::from(limit))
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows.into_iter().map(MessageId::new).collect())
}

/// 聚合任务租约声明
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregateClaim {
    pub account_id: AccountId,
    pub message_id: MessageId,
    pub generation: i64,
    pub lease_token: String,
}
