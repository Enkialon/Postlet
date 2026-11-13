//! SQLite messages 模块函数；连接与事务由调用方持有。

use crate::application::error::PortError;
use crate::domain::entities::{Message, MessageWithRecipients};
use crate::domain::value_objects::{AccountId, GraphImmutableId, MessageId, ThreadId};
use sqlx::SqliteConnection;

pub async fn get(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    id: &MessageId,
) -> Result<Option<MessageWithRecipients>, PortError> {
    let row = sqlx::query("SELECT * FROM messages WHERE account_id = ? AND id = ?")
        .bind(account_id.as_str())
        .bind(id.as_str())
        .fetch_optional(&mut *conn)
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let message = super::rows::message(&row)?;
    let recipients = sqlx::query("SELECT * FROM message_recipients WHERE account_id = ? AND message_id = ? ORDER BY role, seq")
        .bind(account_id.as_str()).bind(id.as_str()).fetch_all(&mut *conn).await?
        .iter().map(super::rows::messagerecipient).collect::<Result<Vec<_>, _>>()?;
    Ok(Some(MessageWithRecipients {
        message,
        recipients,
    }))
}

pub async fn get_by_graph_id(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    graph_id: &GraphImmutableId,
) -> Result<Option<MessageWithRecipients>, PortError> {
    let _ = (conn, account_id, graph_id);
    Err(PortError::Storage {
        message: "尚未实现: repository::messages::get_by_graph_id".into(),
    })
}

pub async fn upsert_with_recipients(
    conn: &mut SqliteConnection,
    message: &MessageWithRecipients,
) -> Result<(), PortError> {
    let _ = (conn, message);
    Err(PortError::Storage {
        message: "尚未实现: repository::messages::upsert_with_recipients".into(),
    })
}

pub async fn mark_read(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_ids: &[MessageId],
    is_read: bool,
) -> Result<Vec<MessageId>, PortError> {
    let _ = (conn, account_id, message_ids, is_read);
    Err(PortError::Storage {
        message: "尚未实现: repository::messages::mark_read".into(),
    })
}

pub async fn mark_deleted_remote(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_ids: &[MessageId],
) -> Result<(), PortError> {
    let _ = (conn, account_id, message_ids);
    Err(PortError::Storage {
        message: "尚未实现: repository::messages::mark_deleted_remote".into(),
    })
}

pub async fn delete_physical(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_ids: &[MessageId],
) -> Result<(), PortError> {
    let _ = (conn, account_id, message_ids);
    Err(PortError::Storage {
        message: "尚未实现: repository::messages::delete_physical".into(),
    })
}

pub async fn list_by_thread(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    thread_id: &ThreadId,
) -> Result<Vec<Message>, PortError> {
    let _ = (conn, account_id, thread_id);
    Err(PortError::Storage {
        message: "尚未实现: repository::messages::list_by_thread".into(),
    })
}
