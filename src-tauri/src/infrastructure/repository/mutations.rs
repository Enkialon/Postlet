//! SQLite mutations 模块函数；连接与事务由调用方持有。

use crate::application::error::PortError;
use crate::domain::value_objects::{AccountId, MessageId, TimestampMs};
use sqlx::SqliteConnection;

use crate::application::dto::PendingMessageMutation;

pub async fn put_read(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_id: &MessageId,
    desired: bool,
) -> Result<(), PortError> {
    let _ = (conn, account_id, message_id, desired);
    Err(PortError::Storage {
        message: "尚未实现: repository::mutations::put_read".into(),
    })
}

pub async fn get(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Option<PendingMessageMutation>, PortError> {
    let _ = (conn, account_id, message_id);
    Err(PortError::Storage {
        message: "尚未实现: repository::mutations::get".into(),
    })
}

pub async fn list_due(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    now: TimestampMs,
    limit: u32,
) -> Result<Vec<PendingMessageMutation>, PortError> {
    let _ = (conn, account_id, now, limit);
    Err(PortError::Storage {
        message: "尚未实现: repository::mutations::list_due".into(),
    })
}

pub async fn acknowledge(
    conn: &mut SqliteConnection,
    mutation: &PendingMessageMutation,
) -> Result<(), PortError> {
    let _ = (conn, mutation);
    Err(PortError::Storage {
        message: "尚未实现: repository::mutations::acknowledge".into(),
    })
}

pub async fn record_failure(
    conn: &mut SqliteConnection,
    mutation: &PendingMessageMutation,
    error: &str,
    available_at: TimestampMs,
) -> Result<(), PortError> {
    let _ = (conn, mutation, error, available_at);
    Err(PortError::Storage {
        message: "尚未实现: repository::mutations::record_failure".into(),
    })
}
