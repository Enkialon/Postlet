//! SQLite pending_message_mutations；连接与事务由调用方持有。

use crate::domain::entities::{PendingMessageMutation, PendingMutationKind};
use crate::infrastructure::error::InfraError;
use crate::domain::value_objects::{AccountId, MessageId, TimestampMs};
use sqlx::{Row, SqliteConnection};

fn map_row(row: &sqlx::sqlite::SqliteRow) -> Result<PendingMessageMutation, InfraError> {
    let kind_raw: String = row.try_get("kind")?;
    let kind = PendingMutationKind::parse(&kind_raw).ok_or_else(|| InfraError::Storage {
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
        target_location_revision: row.try_get("target_location_revision")?,
        target_binding_revision: row.try_get("target_binding_revision")?,
        desired_is_read,
        version: row.try_get("version")?,
        attempts: row.try_get::<i64, _>("attempts")? as i32,
        available_at: row.try_get("available_at")?,
        last_error: row.try_get("last_error")?,
        claim_token: row.try_get("claim_token")?,
        claimed_version: row.try_get("claimed_version")?,
        lease_until: row.try_get("lease_until")?,
        execution_phase: row
            .try_get::<Option<String>, _>("execution_phase")?
            .and_then(|p| crate::domain::entities::MutationPhase::parse(&p)),
        last_error_code: row.try_get("last_error_code")?,
    })
}


/// INSERT…SELECT 未命中（目标无效/已退役）时必须失败，禁止静默成功。
fn require_target_written(rows: u64) -> Result<(), InfraError> {
    if rows == 0 {
        return Err(InfraError::Conflict {
            message: "消息当前没有可用的远端位置，无法记录待推变更".into(),
        });
    }
    Ok(())
}

/// 认领到期意图：写入 claim_token/claimed_version/lease，之后才允许网络写。
#[allow(clippy::too_many_arguments)]
pub async fn claim_due(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    now: TimestampMs,
    lease_until: TimestampMs,
    claim_token: &str,
    limit: u32,
) -> Result<Vec<PendingMessageMutation>, InfraError> {
    let rows = sqlx::query(
        "SELECT * FROM pending_message_mutations
         WHERE account_id = ? AND is_active = 1 AND available_at <= ?
           AND (lease_until IS NULL OR lease_until <= ?)
         ORDER BY available_at, version
         LIMIT ?",
    )
    .bind(account_id.as_str())
    .bind(now)
    .bind(now)
    .bind(i64::from(limit))
    .fetch_all(&mut *conn)
    .await?;

    let mut claimed = Vec::new();
    for row in rows {
        let mutation = map_row(&row)?;
        let result = sqlx::query(
            "UPDATE pending_message_mutations
             SET claim_token = ?, claimed_version = version, lease_until = ?,
                 execution_phase = 'started', last_error_code = NULL
             WHERE account_id = ? AND message_id = ? AND version = ? AND is_active = 1
               AND (lease_until IS NULL OR lease_until <= ?)",
        )
        .bind(claim_token)
        .bind(lease_until)
        .bind(account_id.as_str())
        .bind(mutation.message_id.as_str())
        .bind(mutation.version)
        .bind(now)
        .execute(&mut *conn)
        .await?;
        if result.rows_affected() == 1 {
            let mut m = mutation;
            m.claim_token = Some(claim_token.to_string());
            m.claimed_version = Some(m.version);
            m.lease_until = Some(lease_until);
            m.execution_phase = Some(crate::domain::entities::MutationPhase::Started);
            claimed.push(m);
        }
    }
    Ok(claimed)
}

/// 标记尝试已发出（阶段推进）；条件必须匹配认领标识。
pub async fn mark_attempt_sent(
    conn: &mut SqliteConnection,
    mutation: &PendingMessageMutation,
) -> Result<bool, InfraError> {
    let result = sqlx::query(
        "UPDATE pending_message_mutations SET execution_phase = 'sent'
         WHERE account_id = ? AND message_id = ? AND version = ?
           AND claim_token = ?",
    )
    .bind(mutation.account_id.as_str())
    .bind(mutation.message_id.as_str())
    .bind(mutation.version)
    .bind(mutation.claim_token.as_deref())
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// 记录尝试结果；版本或认领标识不匹配时不确认（旧结果不能覆盖新意图）。
pub async fn record_attempt_result(
    conn: &mut SqliteConnection,
    mutation: &PendingMessageMutation,
    next_attempt_at: Option<TimestampMs>,
    error_code: Option<&str>,
    error: Option<&str>,
) -> Result<bool, InfraError> {
    let result = sqlx::query(
        "UPDATE pending_message_mutations
         SET attempts = attempts + 1, last_error_code = ?, last_error = ?,
             available_at = COALESCE(?, available_at),
             claim_token = NULL, claimed_version = NULL, lease_until = NULL,
             execution_phase = NULL
         WHERE account_id = ? AND message_id = ? AND version = ?
           AND claim_token = ?",
    )
    .bind(error_code)
    .bind(error)
    .bind(next_attempt_at)
    .bind(mutation.account_id.as_str())
    .bind(mutation.message_id.as_str())
    .bind(mutation.version)
    .bind(mutation.claim_token.as_deref())
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// 启动恢复：清理租约，保留意图等待重新核对；不猜测远端已接受。
pub async fn recover_interrupted(
    conn: &mut SqliteConnection,
    now: TimestampMs,
) -> Result<u32, InfraError> {
    let result = sqlx::query(
        "UPDATE pending_message_mutations
         SET claim_token = NULL, claimed_version = NULL, lease_until = NULL,
             execution_phase = NULL, available_at = ?,
             last_error_code = COALESCE(last_error_code, 'interrupted')
         WHERE is_active = 1 AND claim_token IS NOT NULL",
    )
    .bind(now)
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected() as u32)
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
) -> Result<(), InfraError> {
    let now = now_ms();
    let result = sqlx::query(
        "INSERT INTO pending_message_mutations (
            account_id, message_id, kind, desired_is_read, version, attempts, available_at, last_error, target_folder_id, target_remote_key, target_location_revision, target_binding_revision
         ) SELECT m.account_id, m.id, 'read', ?, 1, 0, ?, NULL, m.folder_id, m.remote_key, m.location_revision, f.binding_revision
           FROM messages m JOIN mail_folder_sync_state f ON f.account_id = m.account_id AND f.folder_id = m.folder_id
           WHERE m.account_id = ? AND m.id = ? AND m.location_status = 'present' AND f.is_retired = 0
         ON CONFLICT(account_id, message_id) DO UPDATE SET
            target_folder_id=excluded.target_folder_id,
            target_remote_key=excluded.target_remote_key,
            target_location_revision=excluded.target_location_revision,
            target_binding_revision=excluded.target_binding_revision,
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
    require_target_written(result.rows_affected())
}

/// 写入待推删除（覆盖同消息上的已读待推）。
pub async fn put_delete(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<(), InfraError> {
    let now = now_ms();
    let result = sqlx::query(
        "INSERT INTO pending_message_mutations (
            account_id, message_id, kind, desired_is_read, version, attempts, available_at, last_error, target_folder_id, target_remote_key, target_location_revision, target_binding_revision
         ) SELECT m.account_id, m.id, 'delete', NULL, 1, 0, ?, NULL, m.folder_id, m.remote_key, m.location_revision, f.binding_revision
           FROM messages m JOIN mail_folder_sync_state f ON f.account_id = m.account_id AND f.folder_id = m.folder_id
           WHERE m.account_id = ? AND m.id = ? AND m.location_status = 'present' AND f.is_retired = 0
         ON CONFLICT(account_id, message_id) DO UPDATE SET
            target_folder_id=excluded.target_folder_id,
            target_remote_key=excluded.target_remote_key,
            target_location_revision=excluded.target_location_revision,
            target_binding_revision=excluded.target_binding_revision,
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
    require_target_written(result.rows_affected())
}

pub async fn get(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Option<PendingMessageMutation>, InfraError> {
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
) -> Result<Vec<PendingMessageMutation>, InfraError> {
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

/// 展示只叠加当前有效位置/绑定上的意图；保留但失效的任务不会遮盖真实观测。
pub async fn get_effective(conn: &mut SqliteConnection, account_id: &AccountId, message_id: &MessageId) -> Result<Option<PendingMessageMutation>, InfraError> {
    let row = sqlx::query("SELECT p.* FROM pending_message_mutations p
        JOIN messages m ON m.account_id = p.account_id AND m.id = p.message_id
        JOIN mail_folder_sync_state f ON f.account_id = m.account_id AND f.folder_id = m.folder_id
        WHERE p.account_id = ? AND p.message_id = ? AND p.is_active = 1 AND m.location_status = 'present' AND f.is_retired = 0
          AND m.folder_id = p.target_folder_id AND m.remote_key = p.target_remote_key
          AND m.location_revision = p.target_location_revision AND f.binding_revision = p.target_binding_revision")
        .bind(account_id.as_str()).bind(message_id.as_str()).fetch_optional(conn).await?;
    row.as_ref().map(map_row).transpose()
}

pub async fn acknowledge(
    conn: &mut SqliteConnection,
    mutation: &PendingMessageMutation,
) -> Result<(), InfraError> {
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
        return Err(InfraError::Conflict {
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
) -> Result<(), InfraError> {
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
) -> Result<(), InfraError> {
    sqlx::query("UPDATE pending_message_mutations SET is_active = 0, last_error = ? WHERE account_id = ? AND message_id = ?")
        .bind(error).bind(account_id.as_str()).bind(message_id.as_str()).execute(conn).await?;
    Ok(())
}

pub async fn stop_if_version(conn: &mut SqliteConnection, mutation: &PendingMessageMutation, error: &str) -> Result<(), InfraError> {
    sqlx::query("UPDATE pending_message_mutations SET is_active = 0, last_error = ? WHERE account_id = ? AND message_id = ? AND version = ?")
        .bind(error).bind(mutation.account_id.as_str()).bind(mutation.message_id.as_str()).bind(mutation.version).execute(conn).await?;
    Ok(())
}

/// 校验已登录账号、有效位置、固定目标及绑定修订；只返回已经核对的远端绑定。
pub async fn validate_target(
    conn: &mut SqliteConnection, mutation: &PendingMessageMutation,
    protocol: crate::domain::value_objects::ReceiveProtocol,
) -> Result<Option<String>, InfraError> {
    Ok(sqlx::query_scalar("SELECT f.remote_binding FROM pending_message_mutations p
        JOIN messages m ON m.account_id = p.account_id AND m.id = p.message_id
        JOIN mail_folder_sync_state f ON f.account_id = m.account_id AND f.folder_id = m.folder_id
        JOIN accounts a ON a.id = m.account_id
        WHERE p.account_id = ? AND p.message_id = ? AND p.version = ? AND p.is_active = 1
          AND a.is_signed_in = 1 AND m.location_status = 'present' AND f.is_retired = 0 AND f.protocol = ?
          AND m.folder_id = p.target_folder_id AND m.remote_key = p.target_remote_key
          AND m.location_revision = p.target_location_revision AND f.binding_revision = p.target_binding_revision")
        .bind(mutation.account_id.as_str()).bind(mutation.message_id.as_str()).bind(mutation.version).bind(protocol.as_str())
        .fetch_optional(conn).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    async fn seed(db: &SqlitePool) -> (AccountId, MessageId) {
        sqlx::query("INSERT INTO accounts(id,email,created_at,updated_at) VALUES ('a','a@example.com',1,1)")
            .execute(db)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO mail_folder_sync_state(account_id,folder_id,protocol,remote_binding,binding_revision,is_retired,display_name,sync_enabled,sync_status,updated_at,role,role_source)
             VALUES ('a','inbox','imap','INBOX',1,0,'INBOX',1,'idle',1,'inbox','server')",
        )
        .execute(db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO sessions(account_id,id,kind,normalized_address,created_at,updated_at) VALUES ('a','s1','private','peer@example.com',1,1)",
        )
        .execute(db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO messages(account_id,id,session_id,remote_key,location_status,location_revision,folder_id,subject,body_preview,body_content_type,from_name,is_read,is_draft,is_outbound,has_attachments,importance,local_updated_at)
             VALUES ('a','m1','s1','imap:1:42','present',1,'inbox','s','','text','',0,0,0,0,'normal',1)",
        )
        .execute(db)
        .await
        .unwrap();
        (AccountId::new("a"), MessageId::new("m1"))
    }

    #[tokio::test]
    async fn put_read_fails_when_target_is_not_writable() {
        let db = crate::infrastructure::db::open_memory().await.unwrap();
        let (account, message) = seed(&db).await;
        let mut conn = db.acquire().await.unwrap();

        // 目标文件夹退役 → INSERT…SELECT 不命中，必须报错而不是静默成功
        sqlx::query("UPDATE mail_folder_sync_state SET is_retired = 1 WHERE account_id='a' AND folder_id='inbox'")
            .execute(&mut *conn)
            .await
            .unwrap();
        let err = put_read(&mut conn, &account, &message, true).await.unwrap_err();
        assert!(matches!(err, InfraError::Conflict { .. }));
    }

    #[tokio::test]
    async fn claim_is_exclusive_and_stale_result_cannot_ack_new_intent() {
        let db = crate::infrastructure::db::open_memory().await.unwrap();
        let (account, message) = seed(&db).await;
        let mut conn = db.acquire().await.unwrap();
        put_read(&mut conn, &account, &message, true).await.unwrap();

        let now = now_ms();
        let first = claim_due(&mut conn, &account, now, now + 60_000, "token-1", 8)
            .await
            .unwrap();
        assert_eq!(first.len(), 1);
        // 租约未到期时第二个 worker 不能认领
        let second = claim_due(&mut conn, &account, now, now + 60_000, "token-2", 8)
            .await
            .unwrap();
        assert!(second.is_empty());

        // 新意图递增版本；旧尝试的结果不能确认它
        put_read(&mut conn, &account, &message, false).await.unwrap();
        let stale = first[0].clone();
        let applied = record_attempt_result(&mut conn, &stale, None, None, None)
            .await
            .unwrap();
        assert!(!applied, "stale attempt must not confirm a newer intent");
    }
}
