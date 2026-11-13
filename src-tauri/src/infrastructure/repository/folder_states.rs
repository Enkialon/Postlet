//! SQLite folder_states 模块函数；连接与事务由调用方持有。

use crate::application::error::PortError;
use crate::domain::entities::MailFolderSyncState;
use crate::domain::value_objects::{AccountId, FolderId};
use sqlx::SqliteConnection;

pub async fn get(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    folder_id: &FolderId,
) -> Result<Option<MailFolderSyncState>, PortError> {
    let row = sqlx::query(
        "SELECT * FROM mail_folder_sync_state WHERE account_id = ? AND folder_id = ?",
    )
    .bind(account_id.as_str())
    .bind(folder_id.as_str())
    .fetch_optional(&mut *conn)
    .await?;
    row.as_ref()
        .map(super::rows::mailfoldersyncstate)
        .transpose()
        .map_err(Into::into)
}

pub async fn list_by_account(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
) -> Result<Vec<MailFolderSyncState>, PortError> {
    sqlx::query("SELECT * FROM mail_folder_sync_state WHERE account_id = ? ORDER BY folder_id")
        .bind(account_id.as_str())
        .fetch_all(&mut *conn)
        .await?
        .iter()
        .map(super::rows::mailfoldersyncstate)
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub async fn upsert(
    conn: &mut SqliteConnection,
    state: &MailFolderSyncState,
) -> Result<(), PortError> {
    sqlx::query(
        "INSERT INTO mail_folder_sync_state (
            account_id, folder_id, delta_link, next_link, synced_at, sync_status, last_error, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(account_id, folder_id) DO UPDATE SET
            delta_link=excluded.delta_link,
            next_link=excluded.next_link,
            synced_at=excluded.synced_at,
            sync_status=excluded.sync_status,
            last_error=excluded.last_error,
            updated_at=excluded.updated_at",
    )
    .bind(state.account_id.as_str())
    .bind(state.folder_id.as_str())
    .bind(&state.delta_link)
    .bind(&state.next_link)
    .bind(state.synced_at)
    .bind(state.sync_status.as_str())
    .bind(&state.last_error)
    .bind(state.updated_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn reset_cursors(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    folder_id: &FolderId,
) -> Result<(), PortError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    sqlx::query(
        "UPDATE mail_folder_sync_state
         SET delta_link = NULL, next_link = NULL, updated_at = ?
         WHERE account_id = ? AND folder_id = ?",
    )
    .bind(now)
    .bind(account_id.as_str())
    .bind(folder_id.as_str())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn delete(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    folder_id: &FolderId,
) -> Result<(), PortError> {
    sqlx::query("DELETE FROM mail_folder_sync_state WHERE account_id = ? AND folder_id = ?")
        .bind(account_id.as_str())
        .bind(folder_id.as_str())
        .execute(&mut *conn)
        .await?;
    Ok(())
}
