//! 下载候选与失败重试：位置唯一，远端键对 application 保持不透明。
//!
//! 成功落库走 `mark_downloaded`，失败记录退避后重试；发现候选与发现游标同事务。

use crate::domain::value_objects::{AccountId, FolderId, RemoteKey, TimestampMs};
use crate::infrastructure::error::InfraError;
use sqlx::{Row, SqliteConnection};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateState {
    Pending,
    Downloaded,
    Failed,
}

impl CandidateState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Downloaded => "downloaded",
            Self::Failed => "failed",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        Some(match raw {
            "pending" => Self::Pending,
            "downloaded" => Self::Downloaded,
            "failed" => Self::Failed,
            _ => return None,
        })
    }
}

/// 待下载候选（含重试事实）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadCandidate {
    pub account_id: AccountId,
    pub folder_id: FolderId,
    pub remote_key: RemoteKey,
    pub binding_revision: i64,
    pub generation: Option<String>,
    pub state: CandidateState,
    pub attempts: i32,
    pub next_attempt_at: Option<TimestampMs>,
    pub last_error: Option<String>,
    pub updated_at: TimestampMs,
}

fn map_row(row: &sqlx::sqlite::SqliteRow) -> Result<DownloadCandidate, InfraError> {
    let state_raw: String = row.try_get("state")?;
    let state = CandidateState::parse(&state_raw).ok_or_else(|| InfraError::Storage {
        message: format!("未知下载候选状态: {state_raw}"),
    })?;
    Ok(DownloadCandidate {
        account_id: AccountId::new(row.try_get::<String, _>("account_id")?),
        folder_id: FolderId::new(row.try_get::<String, _>("folder_id")?),
        remote_key: RemoteKey::new(row.try_get::<String, _>("remote_key")?),
        binding_revision: row.try_get("binding_revision")?,
        generation: row.try_get("generation")?,
        state,
        attempts: row.try_get::<i64, _>("attempts")? as i32,
        next_attempt_at: row.try_get("next_attempt_at")?,
        last_error: row.try_get("last_error")?,
        updated_at: row.try_get("updated_at")?,
    })
}

/// 记录发现到的候选；已下载的候选不被重置。
pub async fn record_discovered(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    folder_id: &FolderId,
    remote_key: &RemoteKey,
    binding_revision: i64,
    generation: Option<&str>,
    now: TimestampMs,
) -> Result<(), InfraError> {
    sqlx::query(
        "INSERT INTO mail_download_candidates
         (account_id, folder_id, remote_key, binding_revision, generation, state, attempts, updated_at)
         VALUES (?, ?, ?, ?, ?, 'pending', 0, ?)
         ON CONFLICT(account_id, folder_id, remote_key) DO UPDATE SET
            binding_revision=excluded.binding_revision,
            generation=excluded.generation,
            state=CASE WHEN mail_download_candidates.state = 'downloaded' THEN 'downloaded' ELSE 'pending' END,
            updated_at=excluded.updated_at",
    )
    .bind(account_id.as_str())
    .bind(folder_id.as_str())
    .bind(remote_key.as_str())
    .bind(binding_revision)
    .bind(generation)
    .bind(now)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// 到期候选：跳过冷却中的项，继续更旧候选。
pub async fn list_due(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    folder_id: &FolderId,
    now: TimestampMs,
    limit: u32,
) -> Result<Vec<DownloadCandidate>, InfraError> {
    let rows = sqlx::query(
        "SELECT * FROM mail_download_candidates
         WHERE account_id = ? AND folder_id = ? AND state IN ('pending','failed')
           AND (next_attempt_at IS NULL OR next_attempt_at <= ?)
         ORDER BY updated_at, remote_key
         LIMIT ?",
    )
    .bind(account_id.as_str())
    .bind(folder_id.as_str())
    .bind(now)
    .bind(i64::from(limit))
    .fetch_all(&mut *conn)
    .await?;
    rows.iter().map(map_row).collect()
}

pub async fn mark_downloaded(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    folder_id: &FolderId,
    remote_key: &RemoteKey,
    now: TimestampMs,
) -> Result<(), InfraError> {
    sqlx::query(
        "UPDATE mail_download_candidates
         SET state = 'downloaded', attempts = attempts + 1, next_attempt_at = NULL,
             last_error = NULL, updated_at = ?
         WHERE account_id = ? AND folder_id = ? AND remote_key = ?",
    )
    .bind(now)
    .bind(account_id.as_str())
    .bind(folder_id.as_str())
    .bind(remote_key.as_str())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// 记录失败并退避；不计为已下载。
pub async fn record_failure(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    folder_id: &FolderId,
    remote_key: &RemoteKey,
    error: &str,
    next_attempt_at: Option<TimestampMs>,
    now: TimestampMs,
) -> Result<(), InfraError> {
    sqlx::query(
        "UPDATE mail_download_candidates
         SET state = 'failed', attempts = attempts + 1, last_error = ?,
             next_attempt_at = ?, updated_at = ?
         WHERE account_id = ? AND folder_id = ? AND remote_key = ?",
    )
    .bind(error)
    .bind(next_attempt_at)
    .bind(now)
    .bind(account_id.as_str())
    .bind(folder_id.as_str())
    .bind(remote_key.as_str())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// 代次/绑定失效：丢弃旧候选，避免复活失效位置。
pub async fn invalidate_generation(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    folder_id: &FolderId,
) -> Result<u32, InfraError> {
    let result = sqlx::query(
        "DELETE FROM mail_download_candidates WHERE account_id = ? AND folder_id = ?",
    )
    .bind(account_id.as_str())
    .bind(folder_id.as_str())
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    async fn seed(db: &SqlitePool) {
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
    }

    #[tokio::test]
    async fn failed_candidate_backs_off_without_blocking_others() {
        let db = crate::infrastructure::db::open_memory().await.unwrap();
        seed(&db).await;
        let account = AccountId::new("a");
        let folder = FolderId::new("inbox");
        let mut conn = db.acquire().await.unwrap();

        let old = RemoteKey::new("imap:1:10");
        let newer = RemoteKey::new("imap:1:20");
        for key in [&old, &newer] {
            record_discovered(&mut conn, &account, &folder, key, 1, Some("1"), 1_000)
                .await
                .unwrap();
        }

        // 最新候选失败进入冷却
        record_failure(&mut conn, &account, &folder, &newer, "bad", Some(9_999), 1_000)
            .await
            .unwrap();
        let due = list_due(&mut conn, &account, &folder, 1_000, 10).await.unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].remote_key, old, "cooling candidate must not block older ones");

        // 到期后重新进入候选
        let due_later = list_due(&mut conn, &account, &folder, 10_000, 10).await.unwrap();
        assert_eq!(due_later.len(), 2);

        mark_downloaded(&mut conn, &account, &folder, &old, 2_000).await.unwrap();
        // 已下载不会被重新发现重置
        record_discovered(&mut conn, &account, &folder, &old, 1, Some("1"), 3_000)
            .await
            .unwrap();
        let saved = sqlx::query_scalar::<_, String>(
            "SELECT state FROM mail_download_candidates WHERE account_id='a' AND remote_key='imap:1:10'",
        )
        .fetch_one(&mut *conn)
        .await
        .unwrap();
        assert_eq!(saved, "downloaded");

        // 代次失效清理全部候选
        let removed = invalidate_generation(&mut conn, &account, &folder).await.unwrap();
        assert_eq!(removed, 2);
    }
}
