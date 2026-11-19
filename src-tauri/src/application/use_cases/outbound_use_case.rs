//! 外发：将 PendingLocal 按 send_protocol 推到 Graph / SMTP。

use crate::application::error::AppError;
use crate::domain::entities::{MessageWithRecipients, SendConfig, GraphAuthConfig, PasswordMailConfig};
use crate::domain::value_objects::{AccountId, BodyContentType};
use crate::infrastructure::config::load_azure_auth_config;
use crate::infrastructure::graph::{self, mail::GraphOutgoing};
use crate::infrastructure::paths::AppPaths;
use crate::infrastructure::repository::{accounts, mail_transports, messages};
use crate::infrastructure::smtp::{self, SmtpOutgoing};
use sqlx::SqlitePool;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 执行一次已认领的发送尝试：读取不可变快照并派发，返回明确阶段与结论。
///
/// 只处理队列已认领的 task；不扫描消息状态、不重试、不改本地消息可见性。
pub async fn execute_claimed_attempt(
    db: &SqlitePool,
    paths: &AppPaths,
    operation: &crate::infrastructure::mail::operation::MailOperation,
    account_id: &AccountId,
    task: &crate::domain::entities::SendTask,
) -> Result<crate::domain::mail::SendResult, AppError> {
    let (_account, transport, snapshot) = {
        let mut conn = db.acquire().await?;
        accounts::get(&mut conn, account_id)
            .await
            .map_err(AppError::from)?
            .ok_or_else(|| AppError::not_found(format!("账户不存在: {account_id}")))?;
        let transport = mail_transports::require(&mut conn, account_id)
            .await
            .map_err(AppError::from)?;
        let snapshot = crate::infrastructure::repository::outbound_snapshots::get_snapshot(
            &mut conn,
            account_id,
            &task.snapshot_id,
        )
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::not_found("发送快照不存在"))?;
        ((), transport, snapshot)
    };

    // 快照 MIME 字节原样复用：重试不得改变内容或 Message-ID
    let mime = {
        let mut conn = db.acquire().await?;
        let row = crate::infrastructure::repository::attachments::get_content(
            &mut conn,
            account_id,
            &snapshot.mime_content_id,
        )
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::not_found("发送快照内容缺失"))?;
        drop(conn);
        crate::infrastructure::attachment_store::load_bytes(
            &paths.attachments_dir,
            account_id,
            &row.storage_key,
        )
        .map_err(AppError::from)?
    };

    let envelope: crate::infrastructure::mail::outbound_content::SnapshotEnvelope =
        serde_json::from_str(&snapshot.envelope_json)
            .map_err(|e| AppError::internal(format!("envelope 解析失败: {e}")))?;

    match &transport.send {
        SendConfig::Smtp(config) => {
            let (endpoint, auth) =
                smtp::load_connection(&paths.credentials_dir, config).await?;
            // 取消发生在建立连接/写入前：明确不发送。
            operation
                .checkpoint()
                .map_err(AppError::from)?;
            match operation
                .wait(async {
                    Ok::<_, crate::infrastructure::error::InfraError>(
                        smtp::send_raw_message(&endpoint, &auth, &mime, &envelope).await,
                    )
                })
                .await
            {
                Ok(result) => result.map_err(classify_send_infra_error),
                // 外层超时/取消时无法证明服务器未接受，禁止自动重发。
                Err(_) => Ok(crate::domain::mail::SendResult::new(
                    crate::domain::mail::SendOutcome::Uncertain,
                    crate::domain::mail::SendStage::Data,
                )),
            }
        }
        SendConfig::Graph(auth) => {
            let config = load_azure_auth_config(&paths.config_dir).map_err(AppError::from)?;
            let token = operation
                .wait(graph::auth::get_access_token(&paths.credentials_dir, &config, auth))
                .await
                .map_err(AppError::from)?;
            let body_content_type = if snapshot.html_body.is_some() {
                BodyContentType::Html
            } else {
                BodyContentType::Text
            };
            let body_content = snapshot
                .html_body
                .clone()
                .unwrap_or_else(|| snapshot.text_body.clone());
            let to = envelope
                .to
                .iter()
                .map(|r| (r.address.clone(), r.name.clone()))
                .collect();
            let cc = envelope
                .cc
                .iter()
                .map(|r| (r.address.clone(), r.name.clone()))
                .collect();
            let bcc = envelope
                .bcc
                .iter()
                .map(|r| (r.address.clone(), r.name.clone()))
                .collect();
            operation
                .checkpoint()
                .map_err(AppError::from)?;
            let accepted = match operation
                .wait(async {
                    Ok::<_, crate::infrastructure::error::InfraError>(
                        graph::mail::send_mail(
                    &token,
                    &GraphOutgoing {
                    subject: snapshot.subject.clone(),
                    body_content_type,
                    body_content,
                    to,
                    cc,
                    bcc,
                        save_to_sent_items: true,
                    },
                )
                        .await,
                    )
                })
                .await
            {
                Ok(result) => result,
                // 外层取消/超时后请求可能已到达服务器。
                Err(_) => Ok(crate::domain::mail::SendResult::new(
                    crate::domain::mail::SendOutcome::Uncertain,
                    crate::domain::mail::SendStage::Data,
                )),
            };
            match accepted {
                Ok(result) => Ok(result),
                Err(err) => Err(classify_send_infra_error(err)),
            }
        }
    }
}

/// 已接受结果的本地收尾：发送状态、归档入队和账号版本同一事务提交。
///
/// 网络已成功而本地提交失败时，调用方只能重试本步骤，不能重新发送邮件。
pub async fn complete_accepted_attempt(
    db: &SqlitePool,
    account_id: &AccountId,
    task: &crate::domain::entities::SendTask,
    now: i64,
) -> Result<Option<i64>, AppError> {
    let mut tx = db.begin().await?;
    let applied = crate::infrastructure::repository::send_tasks::record_result(
        &mut tx,
        account_id,
        &task.task_id,
        task.content_version,
        task.attempt_id.as_deref().unwrap_or_default(),
        task.claim_token.as_deref().unwrap_or_default(),
        crate::domain::entities::SendState::Accepted,
        Some(now),
        None,
        None,
        None,
        now,
    )
    .await
    .map_err(AppError::from)?;
    if !applied {
        tx.rollback().await?;
        return Ok(None);
    }
    enqueue_archive(&mut tx, account_id, task, now).await?;
    let version = crate::infrastructure::repository::accounts::bump_change_version(
        &mut tx, account_id,
    )
    .await
    .map_err(AppError::from)?;
    tx.commit().await?;
    Ok(Some(version))
}

/// 按账号策略创建归档任务；与发送接受确认在同一事务内。
async fn enqueue_archive(
    conn: &mut sqlx::SqliteConnection,
    account_id: &AccountId,
    task: &crate::domain::entities::SendTask,
    now: i64,
) -> Result<(), AppError> {
    let policy = {
        let row: Option<(String, String)> = sqlx::query_as(
            "SELECT a.sent_archive_policy, t.receive_protocol
             FROM accounts a
             JOIN account_mail_transport t ON t.account_id = a.id
             WHERE a.id = ?",
        )
        .bind(account_id.as_str())
        .fetch_optional(&mut *conn)
        .await
        .map_err(AppError::from)?;
        let Some((policy, protocol)) = row else {
            return Ok(());
        };
        crate::domain::entities::ArchivePolicy::parse(&policy).unwrap_or_else(|| {
            let protocol = crate::domain::value_objects::ReceiveProtocol::parse(&protocol)
                .unwrap_or(crate::domain::value_objects::ReceiveProtocol::Imap);
            crate::domain::entities::ArchivePolicy::default_for(protocol)
        })
    };
    let state = crate::domain::entities::archive_initial_state(policy);
    let target = if policy == crate::domain::entities::ArchivePolicy::ClientAppend {
        sqlx::query_as::<_, (String, i64)>(
            "SELECT folder_id, binding_revision FROM mail_folder_sync_state
             WHERE account_id = ? AND role = 'sent' AND is_retired = 0
             ORDER BY folder_id LIMIT 1",
        )
        .bind(account_id.as_str())
        .fetch_optional(&mut *conn)
        .await
        .map_err(AppError::from)?
    } else {
        None
    };
    let state = if policy == crate::domain::entities::ArchivePolicy::ClientAppend && target.is_none() {
        crate::domain::entities::ArchiveState::NeedsConfiguration
    } else {
        state
    };
    crate::infrastructure::repository::archive_tasks::enqueue_after_acceptance(
        conn,
        &format!("archive-{}", uuid::Uuid::new_v4().as_hyphenated()),
        account_id,
        &task.task_id,
        &task.snapshot_id,
        state,
        target
            .as_ref()
            .map(|(folder_id, _)| crate::domain::value_objects::FolderId::new(folder_id))
            .as_ref(),
        target.as_ref().map(|(_, revision)| *revision),
        now,
    )
    .await
    .map_err(AppError::from)
}

/// InfraError → 发送结论；用于保留阶段/重试语义
fn classify_send_infra_error(err: crate::infrastructure::error::InfraError) -> AppError {
    err.into()
}
