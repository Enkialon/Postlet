//! SQLite archive_tasks：归档只重试归档，不重发邮件。

use crate::domain::entities::{ArchiveState, ArchiveTask};
use crate::domain::value_objects::{AccountId, FolderId, MessageId, TimestampMs};
use crate::infrastructure::error::InfraError;
use sqlx::{Row, SqliteConnection};

fn map_row(row: &sqlx::sqlite::SqliteRow) -> Result<ArchiveTask, InfraError> {
    let state_raw: String = row.try_get("state")?;
    let state = ArchiveState::parse(&state_raw).ok_or_else(|| InfraError::Storage {
        message: format!("未知归档状态: {state_raw}"),
    })?;
    Ok(ArchiveTask {
        task_id: row.try_get("task_id")?,
        account_id: AccountId::new(row.try_get::<String, _>("account_id")?),
        send_task_id: row.try_get("send_task_id")?,
        snapshot_id: row.try_get("snapshot_id")?,
        state,
        target_folder_id: row
            .try_get::<Option<String>, _>("target_folder_id")?
            .map(FolderId::new),
        target_binding_revision: row.try_get("target_binding_revision")?,
        attempt_id: row.try_get("attempt_id")?,
        claim_token: row.try_get("claim_token")?,
        lease_until: row.try_get("lease_until")?,
        next_attempt_at: row.try_get("next_attempt_at")?,
        last_error: row.try_get("last_error")?,
        confirmed_message_id: row
            .try_get::<Option<String>, _>("confirmed_message_id")?
            .map(MessageId::new),
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

/// 发送接受确认后与发送结果同事务入队。
#[allow(clippy::too_many_arguments)]
pub async fn enqueue_after_acceptance(
    conn: &mut SqliteConnection,
    task_id: &str,
    account_id: &AccountId,
    send_task_id: &str,
    snapshot_id: &str,
    state: ArchiveState,
    target_folder_id: Option<&FolderId>,
    target_binding_revision: Option<i64>,
    now: TimestampMs,
) -> Result<(), InfraError> {
    sqlx::query(
        "INSERT INTO archive_tasks
         (task_id, account_id, send_task_id, snapshot_id, state, target_folder_id,
          target_binding_revision, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(account_id, task_id) DO NOTHING",
    )
    .bind(task_id)
    .bind(account_id.as_str())
    .bind(send_task_id)
    .bind(snapshot_id)
    .bind(state.as_str())
    .bind(target_folder_id.map(|f| f.as_str()))
    .bind(target_binding_revision)
    .bind(now)
    .bind(now)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// 认领到期归档：写 attempt 与租约后才允许网络写。
pub async fn claim_due(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    now: TimestampMs,
    lease_until: TimestampMs,
    claim_token: &str,
    attempt_id: &str,
    limit: u32,
) -> Result<Vec<ArchiveTask>, InfraError> {
    let rows = sqlx::query(
        "SELECT * FROM archive_tasks
         WHERE account_id = ? AND state IN ('append_pending','retryable')
           AND (next_attempt_at IS NULL OR next_attempt_at <= ?)
           AND (lease_until IS NULL OR lease_until <= ?)
         ORDER BY created_at LIMIT ?",
    )
    .bind(account_id.as_str())
    .bind(now)
    .bind(now)
    .bind(i64::from(limit))
    .fetch_all(&mut *conn)
    .await?;

    let mut claims = Vec::new();
    for row in rows {
        let task = map_row(&row)?;
        let updated = sqlx::query(
            "UPDATE archive_tasks
             SET state = 'appending', attempt_id = ?, claim_token = ?, lease_until = ?, updated_at = ?
             WHERE account_id = ? AND task_id = ? AND state IN ('append_pending','retryable')",
        )
        .bind(attempt_id)
        .bind(claim_token)
        .bind(lease_until)
        .bind(now)
        .bind(account_id.as_str())
        .bind(&task.task_id)
        .execute(&mut *conn)
        .await?;
        if updated.rows_affected() == 1 {
            let mut claimed = task;
            claimed.state = ArchiveState::Appending;
            claimed.attempt_id = Some(attempt_id.to_string());
            claimed.claim_token = Some(claim_token.to_string());
            claimed.lease_until = Some(lease_until);
            claimed.updated_at = now;
            claims.push(claimed);
        }
    }
    Ok(claims)
}

/// 记录 APPEND 结果；条件匹配 claim_token 才能确认。
pub async fn record_append_result(
    conn: &mut SqliteConnection,
    task: &ArchiveTask,
    state: ArchiveState,
    next_attempt_at: Option<TimestampMs>,
    error: Option<&str>,
    confirmed_message_id: Option<&MessageId>,
    now: TimestampMs,
) -> Result<bool, InfraError> {
    let result = sqlx::query(
        "UPDATE archive_tasks
         SET state = ?, next_attempt_at = ?, last_error = ?,
             confirmed_message_id = COALESCE(?, confirmed_message_id),
             claim_token = NULL, lease_until = NULL, updated_at = ?
         WHERE account_id = ? AND task_id = ? AND claim_token = ?",
    )
    .bind(state.as_str())
    .bind(next_attempt_at)
    .bind(error)
    .bind(confirmed_message_id.map(|m| m.as_str()))
    .bind(now)
    .bind(task.account_id.as_str())
    .bind(&task.task_id)
    .bind(task.claim_token.as_deref())
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// 服务器副本已出现在确定位置：绑定并确认。
pub async fn confirm_location(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    task_id: &str,
    message_id: &MessageId,
    now: TimestampMs,
) -> Result<bool, InfraError> {
    let result = sqlx::query(
        "UPDATE archive_tasks
         SET state = 'confirmed', confirmed_message_id = ?, updated_at = ?
         WHERE account_id = ? AND task_id = ?
           AND state IN ('waiting_server_copy','retryable','uncertain')",
    )
    .bind(message_id.as_str())
    .bind(now)
    .bind(account_id.as_str())
    .bind(task_id)
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn record_failure(
    conn: &mut SqliteConnection,
    task: &ArchiveTask,
    error: &str,
    next_attempt_at: Option<TimestampMs>,
    now: TimestampMs,
) -> Result<bool, InfraError> {
    record_append_result(
        conn,
        task,
        if next_attempt_at.is_some() {
            ArchiveState::Retryable
        } else {
            ArchiveState::Uncertain
        },
        next_attempt_at,
        Some(error),
        None,
        now,
    )
    .await
}

/// 启动恢复：残留 appending 变 uncertain（可能已 APPEND 成功），不自动重发。
pub async fn recover_interrupted(
    conn: &mut SqliteConnection,
    now: TimestampMs,
) -> Result<u32, InfraError> {
    let result = sqlx::query(
        "UPDATE archive_tasks
         SET state = 'uncertain', claim_token = NULL, lease_until = NULL,
             last_error = COALESCE(last_error, '进程中断，归档结果未确认'), updated_at = ?
         WHERE state = 'appending'",
    )
    .bind(now)
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn interrupted_append_recovers_as_uncertain_and_is_not_reclaimed() {
        let db = crate::infrastructure::db::open_memory().await.unwrap();
        let account = AccountId::new("a");
        let mut conn = db.acquire().await.unwrap();
        sqlx::query("INSERT INTO accounts(id,email,created_at,updated_at) VALUES ('a','a@example.com',1,1)")
            .execute(&mut *conn)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO mail_folder_sync_state(account_id,folder_id,protocol,remote_binding,binding_revision,is_retired,display_name,sync_enabled,sync_status,updated_at,role,role_source)
             VALUES ('a','sent','imap','Sent',1,0,'Sent',1,'idle',1,'sent','server')",
        )
        .execute(&mut *conn)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO attachment_contents(content_id,account_id,storage_key,byte_length,digest,created_at)
             VALUES ('c1','a','c1',1,'d',1)",
        )
        .execute(&mut *conn)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO outbound_snapshots(account_id,snapshot_id,content_version,internet_message_id,date,mime_content_id,envelope_json,subject,text_body,created_at)
             VALUES ('a','s1',1,'<m@x>','d','c1','{}','s','t',1)",
        )
        .execute(&mut *conn)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO sessions(account_id,id,kind,normalized_address,created_at,updated_at)
             VALUES ('a','ses1','private','peer@example.com',1,1)",
        )
        .execute(&mut *conn)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO messages(account_id,id,session_id,subject,body_preview,body_content_type,from_name,is_read,is_draft,is_outbound,has_attachments,importance,local_updated_at)
             VALUES ('a','m1','ses1','s','','text','',0,0,1,0,'normal',1)",
        )
        .execute(&mut *conn)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO send_tasks(task_id,account_id,message_id,content_version,snapshot_id,state,created_at,updated_at)
             VALUES ('send-1','a','m1',1,'s1','accepted',1,1)",
        )
        .execute(&mut *conn)
        .await
        .unwrap();
        enqueue_after_acceptance(
            &mut conn,
            "t1",
            &account,
            "send-1",
            "s1",
            ArchiveState::AppendPending,
            Some(&FolderId::new("sent")),
            Some(1),
            1,
        )
        .await
        .unwrap();

        let claimed = claim_due(&mut conn, &account, 10, 1_000, "c1", "a1", 4)
            .await
            .unwrap();
        assert_eq!(claimed.len(), 1);

        let recovered = recover_interrupted(&mut conn, 20).await.unwrap();
        assert_eq!(recovered, 1);
        // uncertain 不在可认领状态：不会自动再次 APPEND
        let due = claim_due(&mut conn, &account, 30, 2_000, "c2", "a2", 4)
            .await
            .unwrap();
        assert!(due.is_empty());
        let state: String = sqlx::query_scalar(
            "SELECT state FROM archive_tasks WHERE account_id='a' AND task_id='t1'",
        )
        .fetch_one(&mut *conn)
        .await
        .unwrap();
        assert_eq!(state, "uncertain");
    }
}
