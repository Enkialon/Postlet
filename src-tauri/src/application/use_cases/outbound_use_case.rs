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
    account_id: &AccountId,
    task: &crate::domain::entities::SendTask,
) -> Result<crate::domain::mail::SendResult, AppError> {
    use crate::domain::mail::{SendOutcome, SendResult, SendStage};

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
            let result = smtp::send_raw_message(&endpoint, &auth, &mime, &envelope).await?;
            Ok(result)
        }
        SendConfig::Graph(auth) => {
            let config = load_azure_auth_config(&paths.config_dir).map_err(AppError::from)?;
            let token = graph::auth::get_access_token(&paths.credentials_dir, &config, auth)
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
            let accepted = graph::mail::send_mail(
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
            .await;
            match accepted {
                Ok(()) => Ok(SendResult::new(SendOutcome::Accepted, SendStage::Data)),
                Err(err) => Err(classify_send_infra_error(err)),
            }
        }
    }
}

/// InfraError → 发送结论；用于保留阶段/重试语义
fn classify_send_infra_error(err: crate::infrastructure::error::InfraError) -> AppError {
    err.into()
}
