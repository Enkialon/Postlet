//! 外发：将 PendingLocal 按 send_protocol 推到 Graph / SMTP。

use crate::application::error::AppError;
use crate::domain::entities::SendConfig;
use crate::domain::mail::{SendOutcome, SendResult, SendStage};
use crate::domain::value_objects::{AccountId, BodyContentType};
use crate::infrastructure::config::load_azure_auth_config;
use crate::infrastructure::graph::{self, mail::GraphOutgoing};
use crate::infrastructure::mail::operation::{CallOutcome, MailOperation};
use crate::infrastructure::paths::AppPaths;
use crate::infrastructure::repository::{accounts, mail_transports};
use crate::infrastructure::smtp;
use sqlx::SqlitePool;

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
            // 取消发生在建立连接/写入前：明确不发送；写入后只保留协议自身结果。
            run_remote_send(
                operation,
                smtp::send_raw_message(&endpoint, &auth, &mime, &envelope),
            )
            .await
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
            run_remote_send(
                operation,
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
                ),
            )
            .await
        }
    }
}

/// 远端写入统一边界：协议自身结果优先；未开始可恢复；开始后结果未知则不重发。
async fn run_remote_send(
    operation: &MailOperation,
    call: impl std::future::Future<Output = Result<SendResult, crate::infrastructure::error::InfraError>>,
) -> Result<SendResult, AppError> {
    match operation.wait_preserving_result(call).await {
        CallOutcome::Returned(result) => result.map_err(classify_send_infra_error),
        // 取消发生在写入开始前：可证明未提交，按可恢复的暂时失败处理。
        CallOutcome::NotStarted => Ok(SendResult::new(SendOutcome::Transient, SendStage::Connect)),
        // 写入已开始后被取消/截止：没有明确回执，只能待确认，禁止自动重发。
        CallOutcome::Aborted => Ok(SendResult::new(SendOutcome::Uncertain, SendStage::Data)),
    }
}

/// 已接受结果的本地收尾：发送状态与账号版本同一事务提交。
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
    let version = crate::infrastructure::repository::accounts::bump_change_version(
        &mut tx, account_id,
    )
    .await
    .map_err(AppError::from)?;
    tx.commit().await?;
    Ok(Some(version))
}

/// InfraError → 发送结论；用于保留阶段/重试语义
fn classify_send_infra_error(err: crate::infrastructure::error::InfraError) -> AppError {
    err.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::{next_state, SendState};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    fn accepted() -> SendResult {
        SendResult::new(SendOutcome::Accepted, SendStage::Data)
    }

    /// 取消与接受回执同时到达：server 已明确受理即为成功。
    #[tokio::test]
    async fn receipt_is_kept_when_cancelled_at_the_same_time() {
        let operation = MailOperation::new(Duration::from_secs(5));
        let control = operation.clone();
        let result = run_remote_send(&operation, async move {
            control.cancel();
            Ok(accepted())
        })
        .await
        .unwrap();
        assert_eq!(result, accepted());
        assert_eq!(next_state(result.outcome, result.stage), SendState::Accepted);
        assert!(operation.checkpoint().is_err(), "取消事实仍必须保留");
    }

    /// 写入开始前取消：未调用协议、可安全恢复为可重试。
    #[tokio::test]
    async fn cancellation_before_write_is_recoverable_and_skips_the_call() {
        let operation = MailOperation::new(Duration::from_secs(5));
        operation.cancel();
        let called = Arc::new(AtomicBool::new(false));
        let flag = called.clone();
        let result = run_remote_send(&operation, async move {
            flag.store(true, Ordering::SeqCst);
            Ok(accepted())
        })
        .await
        .unwrap();
        assert!(!called.load(Ordering::SeqCst), "未开始的协议调用不得执行");
        assert_eq!(
            result,
            SendResult::new(SendOutcome::Transient, SendStage::Connect)
        );
        assert_eq!(next_state(result.outcome, result.stage), SendState::Retryable);
    }

    /// 预算已耗尽且未开始写入：同样可恢复，不记为结果未知。
    #[tokio::test]
    async fn expired_budget_before_write_is_recoverable() {
        let operation = MailOperation::new(Duration::ZERO);
        let result = run_remote_send(&operation, async { Ok(accepted()) })
            .await
            .unwrap();
        assert_eq!(
            result,
            SendResult::new(SendOutcome::Transient, SendStage::Connect)
        );
    }

    /// 写入开始后被取消：没有明确回执，只能 uncertain，下一次 tick 不重发。
    #[tokio::test]
    async fn cancellation_after_write_started_keeps_unknown_and_is_not_retried() {
        let operation = MailOperation::new(Duration::from_secs(5));
        let control = operation.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let canceller = tokio::spawn(async move {
            started_rx.await.unwrap();
            control.cancel();
        });
        let result = run_remote_send(&operation, async move {
            started_tx.send(()).unwrap();
            tokio::time::sleep(Duration::from_secs(30)).await;
            Ok(accepted())
        })
        .await
        .unwrap();
        canceller.await.unwrap();
        assert_eq!(
            result,
            SendResult::new(SendOutcome::Uncertain, SendStage::Data)
        );
        assert_eq!(next_state(result.outcome, result.stage), SendState::Uncertain);
    }

    /// 协议自身结论原样返回：不确定不得被当成失败，确定失败不得退化为未知。
    #[tokio::test]
    async fn protocol_results_pass_through_unchanged() {
        let operation = MailOperation::new(Duration::from_secs(5));
        let uncertain = run_remote_send(&operation, async {
            Ok(SendResult::new(SendOutcome::Uncertain, SendStage::Data))
        })
        .await
        .unwrap();
        assert_eq!(uncertain.outcome, SendOutcome::Uncertain);

        let error = run_remote_send(&operation, async {
            Err::<SendResult, _>(crate::infrastructure::error::InfraError::Auth {
                message: "SMTP 认证被拒绝".into(),
            })
        })
        .await
        .unwrap_err();
        assert_eq!(error.code, crate::application::error::AppErrorCode::Auth);
    }
}
