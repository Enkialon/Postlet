//! SQLite folder_states 模块函数；连接与事务由调用方持有。

use crate::infrastructure::error::InfraError;
use crate::domain::entities::MailFolderSyncState;
use crate::domain::value_objects::{AccountId, FolderId};
use sqlx::SqliteConnection;

pub async fn get(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    folder_id: &FolderId,
) -> Result<Option<MailFolderSyncState>, InfraError> {
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
) -> Result<Vec<MailFolderSyncState>, InfraError> {
    sqlx::query(
        "SELECT * FROM mail_folder_sync_state WHERE account_id = ?
         ORDER BY display_name COLLATE NOCASE, folder_id",
    )
    .bind(account_id.as_str())
    .fetch_all(&mut *conn)
    .await?
    .iter()
    .map(super::rows::mailfoldersyncstate)
    .collect::<Result<Vec<_>, _>>()
    .map_err(Into::into)
}

pub async fn list_sync_enabled(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
) -> Result<Vec<MailFolderSyncState>, InfraError> {
    sqlx::query(
        "SELECT * FROM mail_folder_sync_state
         WHERE account_id = ? AND sync_enabled = 1
         ORDER BY folder_id",
    )
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
) -> Result<(), InfraError> {
    sqlx::query(
        "INSERT INTO mail_folder_sync_state (
            account_id, folder_id, parent_folder_id, display_name, sync_enabled,
            delta_link, next_link, synced_at, sync_status, last_error, updated_at, generation
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(account_id, folder_id) DO UPDATE SET
            parent_folder_id=excluded.parent_folder_id,
            display_name=excluded.display_name,
            sync_enabled=excluded.sync_enabled,
            generation=excluded.generation,
            delta_link=excluded.delta_link,
            next_link=excluded.next_link,
            synced_at=excluded.synced_at,
            sync_status=excluded.sync_status,
            last_error=excluded.last_error,
            updated_at=excluded.updated_at",
    )
    .bind(state.account_id.as_str())
    .bind(state.folder_id.as_str())
    .bind(state.parent_folder_id.as_ref().map(|f| f.as_str()))
    .bind(&state.display_name)
    .bind(if state.sync_enabled { 1 } else { 0 })
    .bind(&state.delta_link)
    .bind(&state.next_link)
    .bind(state.synced_at)
    .bind(state.sync_status.as_str())
    .bind(&state.last_error)
    .bind(state.updated_at)
    .bind(&state.generation)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// 发现目录时合并：保留已有游标与用户勾选；新夹按 default_enabled。
pub async fn upsert_discovered(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    folder_id: &FolderId,
    parent_folder_id: Option<&FolderId>,
    display_name: &str,
    default_enabled: bool,
    now: i64,
) -> Result<(), InfraError> {
    let existing = get(conn, account_id, folder_id).await?;
    let state = MailFolderSyncState {
        account_id: account_id.clone(),
        folder_id: folder_id.clone(),
        parent_folder_id: parent_folder_id.cloned(),
        display_name: display_name.to_owned(),
        sync_enabled: existing
            .as_ref()
            .map(|e| e.sync_enabled)
            .unwrap_or(default_enabled),
        generation: existing.as_ref().and_then(|e| e.generation.clone()),
        delta_link: existing.as_ref().and_then(|e| e.delta_link.clone()),
        next_link: existing.as_ref().and_then(|e| e.next_link.clone()),
        synced_at: existing.as_ref().and_then(|e| e.synced_at),
        sync_status: existing
            .as_ref()
            .map(|e| e.sync_status)
            .unwrap_or(crate::domain::value_objects::SyncStatus::Idle),
        last_error: existing.as_ref().and_then(|e| e.last_error.clone()),
        updated_at: now,
    };
    upsert(conn, &state).await
}

pub async fn set_sync_enabled(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    folder_id: &FolderId,
    enabled: bool,
    now: i64,
) -> Result<(), InfraError> {
    let result = sqlx::query(
        "UPDATE mail_folder_sync_state
         SET sync_enabled = ?, updated_at = ?
         WHERE account_id = ? AND folder_id = ?",
    )
    .bind(if enabled { 1 } else { 0 })
    .bind(now)
    .bind(account_id.as_str())
    .bind(folder_id.as_str())
    .execute(&mut *conn)
    .await?;
    if result.rows_affected() == 0 {
        return Err(InfraError::Storage {
            message: format!("文件夹不存在: {folder_id}"),
        });
    }
    Ok(())
}

pub async fn reset_cursors(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    folder_id: &FolderId,
) -> Result<(), InfraError> {
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
) -> Result<(), InfraError> {
    sqlx::query("DELETE FROM mail_folder_sync_state WHERE account_id = ? AND folder_id = ?")
        .bind(account_id.as_str())
        .bind(folder_id.as_str())
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// 新发现文件夹默认不勾选；由用户在设置里选择要同步的夹。
pub fn default_sync_enabled(_folder_id: &str, _well_known: Option<&str>) -> bool {
    false
}
