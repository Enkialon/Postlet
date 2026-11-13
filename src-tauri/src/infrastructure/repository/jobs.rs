//! SQLite jobs 模块函数；连接与事务由调用方持有。

use crate::application::error::PortError;
use crate::domain::entities::ConversationAggregateJob;
use sqlx::SqliteConnection;

use crate::application::dto::AggregateClaim;
use crate::domain::value_objects::{AccountId, AggregationOutcome, MessageId, TimestampMs};

pub async fn enqueue_or_reset(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_id: &MessageId,
    available_at: TimestampMs,
) -> Result<(), PortError> {
    let _ = (conn, account_id, message_id, available_at);
    Err(PortError::Storage {
        message: "尚未实现: repository::jobs::enqueue_or_reset".into(),
    })
}

pub async fn claim_batch(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    now: TimestampMs,
    lease_token: &str,
    lease_expires_at: TimestampMs,
    limit: u32,
) -> Result<Vec<ConversationAggregateJob>, PortError> {
    let _ = (conn, account_id, now, lease_token, lease_expires_at, limit);
    Err(PortError::Storage {
        message: "尚未实现: repository::jobs::claim_batch".into(),
    })
}

pub async fn complete(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_id: &MessageId,
    lease_token: &str,
    generation: i64,
    outcome: AggregationOutcome,
) -> Result<(), PortError> {
    let _ = (
        conn,
        account_id,
        message_id,
        lease_token,
        generation,
        outcome,
    );
    Err(PortError::Storage {
        message: "尚未实现: repository::jobs::complete".into(),
    })
}

pub async fn fail_for_retry(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_id: &MessageId,
    lease_token: &str,
    generation: i64,
    error: &str,
    available_at: TimestampMs,
) -> Result<(), PortError> {
    let _ = (
        conn,
        account_id,
        message_id,
        lease_token,
        generation,
        error,
        available_at,
    );
    Err(PortError::Storage {
        message: "尚未实现: repository::jobs::fail_for_retry".into(),
    })
}

pub async fn fail_permanently(
    conn: &mut SqliteConnection,
    claim: &AggregateClaim,
    error: &str,
) -> Result<(), PortError> {
    let _ = (conn, claim, error);
    Err(PortError::Storage {
        message: "尚未实现: repository::jobs::fail_permanently".into(),
    })
}

pub async fn release_expired_leases(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    now: TimestampMs,
) -> Result<u32, PortError> {
    let _ = (conn, account_id, now);
    Err(PortError::Storage {
        message: "尚未实现: repository::jobs::release_expired_leases".into(),
    })
}
