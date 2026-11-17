//! SQLite pending_message_mutations；连接与事务由调用方持有。

use crate::domain::entities::{PendingMessageMutation, PendingMutationKind};
use crate::application::error::PortError;
use crate::domain::value_objects::{AccountId, MessageId, TimestampMs};
use sqlx::{Row, SqliteConnection};

fn map_row(row: &sqlx::sqlite::SqliteRow) -> Result<PendingMessageMutation, PortError> {
    let kind_raw: String = row.try_get("kind")?;
    let kind = PendingMutationKind::parse(&kind_raw).ok_or_else(|| PortError::Storage {
        message: format!("未知 pending mutation kind: {kind_raw}"),
    })?;
    let desired_is_read = row
        .try_get::<Option<i64>, _>("desired_is_read")?
        .map(|v| v != 0);
    Ok(PendingMessageMutation {
        account_id: AccountId::new(row.try_get::<String, _>("account_id")?),
        message_id: MessageId::new(row.try_get::<String, _>("message_id")?),
        kind,
        is_active: row.try_get("is_active")?,
        target_folder_id: crate::domain::value_objects::FolderId::new(row.try_get::<String, _>("target_folder_id")?),
        target_remote_key: crate::domain::value_objects::RemoteKey::new(row.try_get::<String, _>("target_remote_key")?),
        desired_is_read,
        version: row.try_get("version")?,
        attempts: row.try_get::<i64, _>("attempts")? as i32,
        available_at: row.try_get("available_at")?,
        last_error: row.try_get("last_error")?,
    })
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 写入或更新待推已读；同消息升 version 并重置 attempts。
pub async fn put_read(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_id: &MessageId,
    desired: bool,
) -> Result<(), PortError> {
    let now = now_ms();
    sqlx::query(
        "INSERT INTO pending_message_mutations (
            account_id, message_id, kind, desired_is_read, version, attempts, available_at, last_error, target_folder_id, target_remote_key
         ) SELECT account_id, id, 'read', ?, 1, 0, ?, NULL, folder_id, remote_key
           FROM messages WHERE account_id = ? AND id = ? AND folder_id IS NOT NULL AND remote_key IS NOT NULL
         ON CONFLICT(account_id, message_id) DO UPDATE SET
            target_folder_id=excluded.target_folder_id,
            target_remote_key=excluded.target_remote_key,
            kind='read',
            is_active=1,
            desired_is_read=excluded.desired_is_read,
            version=pending_message_mutations.version + 1,
            attempts=0,
            available_at=excluded.available_at,
            last_error=NULL",
    )
    .bind(if desired { 1 } else { 0 })
    .bind(now)
    .bind(account_id.as_str())
    .bind(message_id.as_str())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// 写入待推删除（覆盖同消息上的已读待推）。
pub async fn put_delete(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<(), PortError> {
    let now = now_ms();
    sqlx::query(
        "INSERT INTO pending_message_mutations (
            account_id, message_id, kind, desired_is_read, version, attempts, available_at, last_error, target_folder_id, target_remote_key
         ) SELECT account_id, id, 'delete', NULL, 1, 0, ?, NULL, folder_id, remote_key
           FROM messages WHERE account_id = ? AND id = ? AND folder_id IS NOT NULL AND remote_key IS NOT NULL
         ON CONFLICT(account_id, message_id) DO UPDATE SET
            target_folder_id=excluded.target_folder_id,
            target_remote_key=excluded.target_remote_key,
            kind='delete',
            is_active=1,
            desired_is_read=NULL,
            version=pending_message_mutations.version + 1,
            attempts=0,
            available_at=excluded.available_at,
            last_error=NULL",
    )
    .bind(now)
    .bind(account_id.as_str())
    .bind(message_id.as_str())
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
         WHERE account_id = ? AND is_active = 1 AND available_at <= ?
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
    next_attempt_at: Option<TimestampMs>,
) -> Result<(), PortError> {
    sqlx::query(
        "UPDATE pending_message_mutations
         SET attempts = attempts + 1, last_error = ?, available_at = COALESCE(?, available_at), is_active = ?
         WHERE account_id = ? AND message_id = ? AND version = ?",
    )
    .bind(error)
    .bind(next_attempt_at)
    .bind(next_attempt_at.is_some())
    .bind(mutation.account_id.as_str())
    .bind(mutation.message_id.as_str())
    .bind(mutation.version)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// 停止引用失效位置的任务，保留错误记录供检查；不迁移到任何新位置。
pub async fn stop_for_message(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_id: &MessageId,
    error: &str,
) -> Result<(), PortError> {
    sqlx::query("UPDATE pending_message_mutations SET is_active = 0, last_error = ? WHERE account_id = ? AND message_id = ?")
        .bind(error).bind(account_id.as_str()).bind(message_id.as_str()).execute(conn).await?;
    Ok(())
}
