//! SQLite pending_message_mutations；连接与事务由调用方持有。

use crate::application::dto::PendingMessageMutation;
use crate::application::error::PortError;
use crate::domain::value_objects::{AccountId, MessageId, TimestampMs};
use sqlx::{Row, SqliteConnection};

fn map_row(row: &sqlx::sqlite::SqliteRow) -> Result<PendingMessageMutation, PortError> {
    Ok(PendingMessageMutation {
        account_id: AccountId::new(row.try_get::<String, _>("account_id")?),
        message_id: MessageId::new(row.try_get::<String, _>("message_id")?),
        desired_is_read: row.try_get::<i64, _>("desired_is_read")? != 0,
        version: row.try_get("version")?,
        attempts: row.try_get::<i64, _>("attempts")? as i32,
        available_at: row.try_get("available_at")?,
        last_error: row.try_get("last_error")?,
    })
}

/// 写入或更新待推已读；同消息升 version 并重置 attempts。
pub async fn put_read(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_id: &MessageId,
    desired: bool,
) -> Result<(), PortError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    sqlx::query(
        "INSERT INTO pending_message_mutations (
            account_id, message_id, desired_is_read, version, attempts, available_at, last_error
         ) VALUES (?, ?, ?, 1, 0, ?, NULL)
         ON CONFLICT(account_id, message_id) DO UPDATE SET
            desired_is_read=excluded.desired_is_read,
            version=pending_message_mutations.version + 1,
            attempts=0,
            available_at=excluded.available_at,
            last_error=NULL",
    )
    .bind(account_id.as_str())
    .bind(message_id.as_str())
    .bind(if desired { 1 } else { 0 })
    .bind(now)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn get(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Option<PendingMessageMutation>, PortError> {
    let row = sqlx::query(
        "SELECT * FROM pending_message_mutations WHERE account_id = ? AND message_id = ?",
    )
    .bind(account_id.as_str())
    .bind(message_id.as_str())
    .fetch_optional(&mut *conn)
    .await?;
    row.as_ref().map(map_row).transpose()
}

pub async fn list_due(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    now: TimestampMs,
    limit: u32,
) -> Result<Vec<PendingMessageMutation>, PortError> {
    let rows = sqlx::query(
        "SELECT * FROM pending_message_mutations
         WHERE account_id = ? AND available_at <= ?
         ORDER BY available_at ASC, version ASC
         LIMIT ?",
    )
    .bind(account_id.as_str())
    .bind(now)
    .bind(i64::from(limit))
    .fetch_all(&mut *conn)
    .await?;
    rows.iter().map(map_row).collect()
}

pub async fn acknowledge(
    conn: &mut SqliteConnection,
    mutation: &PendingMessageMutation,
) -> Result<(), PortError> {
    let result = sqlx::query(
        "DELETE FROM pending_message_mutations
         WHERE account_id = ? AND message_id = ? AND version = ?",
    )
    .bind(mutation.account_id.as_str())
    .bind(mutation.message_id.as_str())
    .bind(mutation.version)
    .execute(&mut *conn)
    .await?;
    if result.rows_affected() == 0 {
        return Err(PortError::Conflict {
            message: "pending mutation 版本已变更，跳过确认".into(),
        });
    }
    Ok(())
}

pub async fn record_failure(
    conn: &mut SqliteConnection,
    mutation: &PendingMessageMutation,
    error: &str,
    available_at: TimestampMs,
) -> Result<(), PortError> {
    sqlx::query(
        "UPDATE pending_message_mutations
         SET attempts = attempts + 1, last_error = ?, available_at = ?
         WHERE account_id = ? AND message_id = ? AND version = ?",
    )
    .bind(error)
    .bind(available_at)
    .bind(mutation.account_id.as_str())
    .bind(mutation.message_id.as_str())
    .bind(mutation.version)
    .execute(&mut *conn)
    .await?;
    Ok(())
}
