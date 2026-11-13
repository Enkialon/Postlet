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
    let _ = (conn, account_id, folder_id);
    Err(PortError::Storage {
        message: "尚未实现: repository::folder_states::get".into(),
    })
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
    let _ = (conn, state);
    Err(PortError::Storage {
        message: "尚未实现: repository::folder_states::upsert".into(),
    })
}

pub async fn reset_cursors(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    folder_id: &FolderId,
) -> Result<(), PortError> {
    let _ = (conn, account_id, folder_id);
    Err(PortError::Storage {
        message: "尚未实现: repository::folder_states::reset_cursors".into(),
    })
}

pub async fn delete(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    folder_id: &FolderId,
) -> Result<(), PortError> {
    let _ = (conn, account_id, folder_id);
    Err(PortError::Storage {
        message: "尚未实现: repository::folder_states::delete".into(),
    })
}
