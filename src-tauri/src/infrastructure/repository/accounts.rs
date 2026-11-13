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
    let row = sqlx::query("SELECT * FROM accounts WHERE email = ?")
        .bind(email.as_str())
        .fetch_optional(&mut *conn)
        .await?;
    row.as_ref()
        .map(super::rows::account)
        .transpose()
        .map_err(Into::into)
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
    let aliases_json = serde_json::to_string(
        &account
            .aliases
            .iter()
            .map(|a| a.as_str())
            .collect::<Vec<_>>(),
    )
    .map_err(|e| PortError::Storage {
        message: format!("aliases_json 序列化失败: {e}"),
    })?;
    sqlx::query(
        "INSERT INTO accounts (
            id, email, display_name, aliases_json, tenant_id, home_account_id,
            contact_delta_link, mail_synced_at, contact_synced_at, mail_sync_status,
            mail_sync_error, show_group_messages_in_contact_conversation, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
            email=excluded.email,
            display_name=excluded.display_name,
            aliases_json=excluded.aliases_json,
            tenant_id=excluded.tenant_id,
            home_account_id=excluded.home_account_id,
            contact_delta_link=excluded.contact_delta_link,
            mail_synced_at=excluded.mail_synced_at,
            contact_synced_at=excluded.contact_synced_at,
            mail_sync_status=excluded.mail_sync_status,
            mail_sync_error=excluded.mail_sync_error,
            show_group_messages_in_contact_conversation=excluded.show_group_messages_in_contact_conversation,
            updated_at=excluded.updated_at",
    )
    .bind(account.id.as_str())
    .bind(account.email.as_str())
    .bind(&account.display_name)
    .bind(&aliases_json)
    .bind(&account.tenant_id)
    .bind(&account.home_account_id)
    .bind(&account.contact_delta_link)
    .bind(account.mail_synced_at)
    .bind(account.contact_synced_at)
    .bind(account.mail_sync_status.as_str())
    .bind(&account.mail_sync_error)
    .bind(account.show_group_messages_in_contact_conversation)
    .bind(account.created_at)
    .bind(account.updated_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn update_mail_sync_meta(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    status: SyncStatus,
    error: Option<&str>,
    mail_synced_at: Option<TimestampMs>,
) -> Result<(), PortError> {
    sqlx::query(
        "UPDATE accounts SET mail_sync_status = ?, mail_sync_error = ?, mail_synced_at = COALESCE(?, mail_synced_at), updated_at = ?
         WHERE id = ?",
    )
    .bind(status.as_str())
    .bind(error)
    .bind(mail_synced_at)
    .bind(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0),
    )
    .bind(account_id.as_str())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn update_contact_sync_meta(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    contact_delta_link: Option<&str>,
    contact_synced_at: Option<TimestampMs>,
) -> Result<(), PortError> {
    sqlx::query(
        "UPDATE accounts SET contact_delta_link = ?, contact_synced_at = COALESCE(?, contact_synced_at), updated_at = ?
         WHERE id = ?",
    )
    .bind(contact_delta_link)
    .bind(contact_synced_at)
    .bind(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0),
    )
    .bind(account_id.as_str())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn update_display_settings(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    show_group_messages_in_contact_conversation: bool,
) -> Result<(), PortError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let result = sqlx::query(
        "UPDATE accounts SET show_group_messages_in_contact_conversation = ?, updated_at = ? WHERE id = ?",
    )
    .bind(show_group_messages_in_contact_conversation)
    .bind(now)
    .bind(account_id.as_str())
    .execute(&mut *conn)
    .await?;
    if result.rows_affected() == 0 {
        return Err(PortError::Conflict {
            message: format!("账户不存在: {account_id}"),
        });
    }
    Ok(())
}

pub async fn delete(conn: &mut SqliteConnection, id: &AccountId) -> Result<(), PortError> {
    sqlx::query("DELETE FROM accounts WHERE id = ?")
        .bind(id.as_str())
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// 删除全部账户（级联清库，供演示工作区重装）
pub async fn delete_all(conn: &mut SqliteConnection) -> Result<(), PortError> {
    sqlx::query("DELETE FROM accounts")
        .execute(&mut *conn)
        .await?;
    Ok(())
}
