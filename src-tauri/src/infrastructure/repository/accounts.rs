//! SQLite accounts 模块函数；连接与事务由调用方持有。

use crate::application::error::PortError;
use crate::domain::entities::Account;
use crate::domain::value_objects::{AccountId, EmailAddress, SyncStatus, TimestampMs};
use sqlx::SqliteConnection;

pub async fn get(
    conn: &mut SqliteConnection,
    id: &AccountId,
) -> Result<Option<Account>, PortError> {
    let row = sqlx::query("SELECT * FROM accounts WHERE id = ?")
        .bind(id.as_str())
        .fetch_optional(&mut *conn)
        .await?;
    row.as_ref()
        .map(super::rows::account)
        .transpose()
        .map_err(Into::into)
}

pub async fn get_by_email(
    conn: &mut SqliteConnection,
    email: &EmailAddress,
) -> Result<Option<Account>, PortError> {
    let _ = (conn, email);
    Err(PortError::Storage {
        message: "尚未实现: repository::accounts::get_by_email".into(),
    })
}

pub async fn list(conn: &mut SqliteConnection) -> Result<Vec<Account>, PortError> {
    sqlx::query("SELECT * FROM accounts ORDER BY id")
        .fetch_all(&mut *conn)
        .await?
        .iter()
        .map(super::rows::account)
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub async fn upsert(conn: &mut SqliteConnection, account: &Account) -> Result<(), PortError> {
    let _ = (conn, account);
    Err(PortError::Storage {
        message: "尚未实现: repository::accounts::upsert".into(),
    })
}

pub async fn update_mail_sync_meta(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    status: SyncStatus,
    error: Option<&str>,
    mail_synced_at: Option<TimestampMs>,
) -> Result<(), PortError> {
    let _ = (conn, account_id, status, error, mail_synced_at);
    Err(PortError::Storage {
        message: "尚未实现: repository::accounts::update_mail_sync_meta".into(),
    })
}

pub async fn update_contact_sync_meta(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    contact_delta_link: Option<&str>,
    contact_synced_at: Option<TimestampMs>,
) -> Result<(), PortError> {
    let _ = (conn, account_id, contact_delta_link, contact_synced_at);
    Err(PortError::Storage {
        message: "尚未实现: repository::accounts::update_contact_sync_meta".into(),
    })
}

pub async fn update_display_settings(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    show_group_messages_in_contact_conversation: bool,
) -> Result<(), PortError> {
    let _ = (
        conn,
        account_id,
        show_group_messages_in_contact_conversation,
    );
    Err(PortError::Storage {
        message: "尚未实现: repository::accounts::update_display_settings".into(),
    })
}

pub async fn delete(conn: &mut SqliteConnection, id: &AccountId) -> Result<(), PortError> {
    let _ = (conn, id);
    Err(PortError::Storage {
        message: "尚未实现: repository::accounts::delete".into(),
    })
}
