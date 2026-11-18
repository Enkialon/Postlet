//! 归档 worker：只重试归档，绝不重发邮件。
//!
//! server_saved 等服务器副本出现（由同步 confirm_location 收敛）；
//! client_append 用持久化 MIME 执行 APPEND，结果未知记 uncertain 不重试。

use crate::application::error::AppError;
use crate::domain::entities::{ArchiveState, ArchiveTask};
use crate::domain::value_objects::{AccountId, ReceiveProtocol};
use crate::infrastructure::mail::operation::MailOperation;
use crate::infrastructure::paths::AppPaths;
use crate::infrastructure::repository::{accounts, archive_tasks, attachments, mail_transports};
use crate::runtime::MailSyncGate;
use sqlx::SqlitePool;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const TICK: Duration = Duration::from_secs(10);
const LEASE: Duration = Duration::from_secs(5 * 60);

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn spawn(db: SqlitePool, paths: AppPaths, gate: MailSyncGate) {
    tauri::async_runtime::spawn(async move {
        if let Ok(mut conn) = db.acquire().await {
            let _ = archive_tasks::recover_interrupted(&mut conn, now_ms()).await;
        }
        loop {
            if let Err(error) = tick_once(&db, &paths, &gate).await {
                eprintln!("归档队列派发失败: {}", error.message);
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

async fn tick_once(db: &SqlitePool, paths: &AppPaths, gate: &MailSyncGate) -> Result<(), AppError> {
    let account_ids = {
        let mut conn = db.acquire().await?;
        accounts::list(&mut conn)
            .await
            .map_err(AppError::from)?
            .into_iter()
            .map(|a| a.id)
            .collect::<Vec<_>>()
    };
    for account_id in account_ids {
        // 先取得账号许可再认领；许可覆盖网络与归档结果落库整个尝试。
        let operation = MailOperation::new(Duration::from_secs(120));
        let Some(permit) =
            gate.try_acquire_operation(account_id.as_str(), Some(operation))
        else {
            continue;
        };
        let claimed = {
            let mut conn = db.acquire().await?;
            let now = now_ms();
            archive_tasks::claim_due(
                &mut conn,
                &account_id,
                now,
                now + LEASE.as_millis() as i64,
                &uuid::Uuid::new_v4().to_string(),
                &uuid::Uuid::new_v4().to_string(),
                4,
            )
            .await
            .map_err(AppError::from)?
        };
        if claimed.is_empty() {
            continue;
        }
        let db2 = db.clone();
        let paths2 = paths.clone();
        let account2 = account_id.clone();
        tauri::async_runtime::spawn(async move {
            let _permit = permit;
            for task in claimed {
                let _ = run_attempt(&db2, &paths2, &account2, &task).await;
            }
        });
    }
    Ok(())
}

async fn run_attempt(
    db: &SqlitePool,
    paths: &AppPaths,
    account_id: &AccountId,
    task: &ArchiveTask,
) -> Result<(), AppError> {
    let result = append_once(db, paths, account_id, task).await;

    let mut conn = db.acquire().await?;
    let now = now_ms();
    match result {
        Ok(Some(true)) => {
            // APPEND 已确认写入；本地位置由后续同步发现并 confirm_location
            archive_tasks::record_append_result(
                &mut conn,
                task,
                ArchiveState::Confirmed,
                None,
                None,
                None,
                now,
            )
            .await
            .map_err(AppError::from)?;
        }
        Ok(Some(false)) => {
            archive_tasks::record_failure(
                &mut conn,
                task,
                "APPEND 被拒绝",
                Some(now + 60_000),
                now,
            )
            .await
            .map_err(AppError::from)?;
        }
        Ok(None) => {
            archive_tasks::record_append_result(
                &mut conn,
                task,
                ArchiveState::Uncertain,
                None,
                Some("APPEND 结果未确认"),
                None,
                now,
            )
            .await
            .map_err(AppError::from)?;
        }
        Err(error) => {
            if matches!(
                error.code,
                crate::application::error::AppErrorCode::Validation
                    | crate::application::error::AppErrorCode::Conflict
            ) {
                // 目标文件夹退役/修订不匹配：停止旧目标，等待重新配置。
                archive_tasks::record_append_result(
                    &mut conn,
                    task,
                    ArchiveState::NeedsConfiguration,
                    None,
                    Some(&error.message),
                    None,
                    now,
                )
                .await
                .map_err(AppError::from)?;
            } else {
                archive_tasks::record_failure(
                    &mut conn,
                    task,
                    &error.message,
                    error.retryable.then(|| now + 60_000),
                    now,
                )
                .await
                .map_err(AppError::from)?;
            }
        }
    }
    Ok(())
}

/// 返回 Some(true)=已写入，Some(false)=被拒绝，None=结果未知
async fn append_once(
    db: &SqlitePool,
    paths: &AppPaths,
    account_id: &AccountId,
    task: &ArchiveTask,
) -> Result<Option<bool>, AppError> {
    let (transport, mime) = {
        let mut conn = db.acquire().await?;
        let transport = mail_transports::require(&mut conn, account_id)
            .await
            .map_err(AppError::from)?;
        if transport.receive.protocol() != ReceiveProtocol::Imap {
            // 服务器保存策略：等同步确认副本，不在客户端 APPEND
            return Ok(Some(true));
        }
        let snapshot = crate::infrastructure::repository::outbound_snapshots::get_snapshot(
            &mut conn,
            account_id,
            &task.snapshot_id,
        )
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::not_found("归档快照不存在"))?;
        let row = attachments::get_content(&mut conn, account_id, &snapshot.mime_content_id)
            .await
            .map_err(AppError::from)?
            .ok_or_else(|| AppError::not_found("归档快照内容缺失"))?;
        let mime = crate::infrastructure::attachment_store::load_bytes(
            &paths.attachments_dir,
            account_id,
            &row.storage_key,
        )
        .map_err(AppError::from)?;
        (transport, mime)
    };

    let crate::domain::entities::ReceiveConfig::Imap(config) = &transport.receive else {
        return Ok(Some(true));
    };
    let destination = task
        .target_folder_id
        .as_ref()
        .ok_or_else(|| AppError::validation("缺少 Sent 归档文件夹"))?;
    // 本地 FolderId 不是协议端点：执行时必须重新解析有效、未退役且修订一致的远端绑定。
    let binding = {
        let mut conn = db.acquire().await?;
        let row: Option<(String, i64, bool)> = sqlx::query_as(
            "SELECT remote_binding, binding_revision, is_retired
             FROM mail_folder_sync_state
             WHERE account_id = ? AND folder_id = ? AND protocol = 'imap'",
        )
        .bind(account_id.as_str())
        .bind(destination.as_str())
        .fetch_optional(&mut *conn)
        .await
        .map_err(AppError::from)?;
        resolve_remote_binding(row, task.target_binding_revision)?
    };
    let (endpoint, auth) =
        crate::infrastructure::imap::load_connection(&paths.credentials_dir, config).await?;
    let accepted = crate::infrastructure::imap::append_message_async(
        &endpoint,
        &auth,
        &binding,
        &mime,
    )
    .await?;
    Ok(accepted)
}

fn resolve_remote_binding(
    row: Option<(String, i64, bool)>,
    expected_revision: Option<i64>,
) -> Result<String, AppError> {
    match row {
        Some((remote_binding, revision, false))
            if Some(revision) == expected_revision =>
        {
            Ok(remote_binding)
        }
        Some(_) => Err(AppError::conflict(
            "Sent 目标文件夹已退役或绑定修订已改变，已停止旧归档任务",
        )),
        None => Err(AppError::validation(
            "Sent 目标文件夹不存在或协议不匹配，请重新配置归档策略",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_uses_remote_binding_only_when_revision_matches() {
        assert_eq!(
            resolve_remote_binding(Some(("Sent Items".into(), 3, false)), Some(3)).unwrap(),
            "Sent Items"
        );
        assert!(matches!(
            resolve_remote_binding(Some(("Sent Items".into(), 4, false)), Some(3)),
            Err(AppError { code: crate::application::error::AppErrorCode::Conflict, .. })
        ));
        assert!(matches!(
            resolve_remote_binding(Some(("Sent Items".into(), 3, true)), Some(3)),
            Err(AppError { code: crate::application::error::AppErrorCode::Conflict, .. })
        ));
        assert!(matches!(
            resolve_remote_binding(None, Some(3)),
            Err(AppError { code: crate::application::error::AppErrorCode::Validation, .. })
        ));
    }
}
