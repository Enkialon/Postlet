//! Graph Remove 待核对记录。

use crate::application::error::PortError;
use crate::domain::value_objects::{AccountId, FolderId, GraphImmutableId, TimestampMs};
use sqlx::{Row, SqliteConnection};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocatePending {
    pub account_id: AccountId,
    pub graph_id: GraphImmutableId,
    pub source_folder_id: FolderId,
    pub revision: i64,
    pub available_at: TimestampMs,
    pub last_error: Option<String>,
    pub updated_at: TimestampMs,
}

fn map_row(row: &sqlx::sqlite::SqliteRow) -> Result<LocatePending, PortError> {
    Ok(LocatePending {
        account_id: AccountId::new(row.try_get::<String, _>("account_id")?),
        graph_id: GraphImmutableId::new(row.try_get::<String, _>("graph_id")?),
        source_folder_id: FolderId::new(row.try_get::<String, _>("source_folder_id")?),
        revision: row.try_get("revision")?,
        available_at: row.try_get("available_at")?,
        last_error: row.try_get("last_error")?,
        updated_at: row.try_get("updated_at")?,
    })
}

/// 入队或升 revision；同 graph_id 重复 Remove 会 bump revision。
pub async fn enqueue(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    graph_id: &GraphImmutableId,
    source_folder_id: &FolderId,
    now: TimestampMs,
) -> Result<(), PortError> {
    sqlx::query(
        "INSERT INTO message_locate_pending (
            account_id, graph_id, source_folder_id, revision, available_at, last_error, updated_at
         ) VALUES (?, ?, ?, 1, ?, NULL, ?)
         ON CONFLICT(account_id, graph_id) DO UPDATE SET
            source_folder_id=excluded.source_folder_id,
            revision=message_locate_pending.revision + 1,
            available_at=excluded.available_at,
            last_error=NULL,
            updated_at=excluded.updated_at",
    )
    .bind(account_id.as_str())
    .bind(graph_id.as_str())
    .bind(source_folder_id.as_str())
    .bind(now)
    .bind(now)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn list_due(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    now: TimestampMs,
    limit: u32,
) -> Result<Vec<LocatePending>, PortError> {
    let rows = sqlx::query(
        "SELECT * FROM message_locate_pending
         WHERE account_id = ? AND available_at <= ?
         ORDER BY available_at ASC, revision ASC
         LIMIT ?",
    )
    .bind(account_id.as_str())
    .bind(now)
    .bind(i64::from(limit))
    .fetch_all(&mut *conn)
    .await?;
    rows.iter().map(map_row).collect()
}

pub async fn get(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    graph_id: &GraphImmutableId,
) -> Result<Option<LocatePending>, PortError> {
    let row = sqlx::query(
        "SELECT * FROM message_locate_pending WHERE account_id = ? AND graph_id = ?",
    )
    .bind(account_id.as_str())
    .bind(graph_id.as_str())
    .fetch_optional(&mut *conn)
    .await?;
    row.as_ref().map(map_row).transpose()
}

/// revision 仍匹配时删除核对记录。
pub async fn complete_if_revision(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    graph_id: &GraphImmutableId,
    revision: i64,
) -> Result<bool, PortError> {
    let result = sqlx::query(
        "DELETE FROM message_locate_pending
         WHERE account_id = ? AND graph_id = ? AND revision = ?",
    )
    .bind(account_id.as_str())
    .bind(graph_id.as_str())
    .bind(revision)
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn record_failure(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    graph_id: &GraphImmutableId,
    revision: i64,
    error: &str,
    available_at: TimestampMs,
    now: TimestampMs,
) -> Result<(), PortError> {
    sqlx::query(
        "UPDATE message_locate_pending
         SET last_error = ?, available_at = ?, updated_at = ?
         WHERE account_id = ? AND graph_id = ? AND revision = ?",
    )
    .bind(error)
    .bind(available_at)
    .bind(now)
    .bind(account_id.as_str())
    .bind(graph_id.as_str())
    .bind(revision)
    .execute(&mut *conn)
    .await?;
    Ok(())
}
