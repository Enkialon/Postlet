//! 邮件用例
use crate::infrastructure::repository::{conversations, messages};
use sqlx::SqlitePool;

use crate::application::dto::MarkMessagesReadCommand;
use crate::application::error::AppError;
use crate::domain::entities::MessageWithRecipients;
use crate::domain::value_objects::{AccountId, MessageId};
use std::collections::HashSet;

/// 在指定账户内读取本地邮件及收件人；不存在时返回未找到错误。
///
/// 不主动补取正文，也不因读取而自动标记已读。
pub async fn get_message(
    db: &SqlitePool,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<MessageWithRecipients, AppError> {
    let mut conn = db.acquire().await?;
    messages::get(&mut conn, account_id, message_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("邮件不存在: {message_id}")))
}

/// 确保邮件正文已加载。
///
/// 本阶段不接 Graph：直接返回本地行；`body_content == None` 表示仍未加载。
pub async fn ensure_message_body(
    db: &SqlitePool,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<MessageWithRecipients, AppError> {
    get_message(db, account_id, message_id).await
}

/// 批量修改指定账户邮件的已读状态，并重算相关会话未读数。
///
/// 本阶段只写本地库，不排队推送到 Outlook。
pub async fn mark_read(db: &SqlitePool, cmd: MarkMessagesReadCommand) -> Result<(), AppError> {
    let mut tx = db.begin().await?;
    let changed =
        messages::mark_read(&mut tx, &cmd.account_id, &cmd.message_ids, cmd.is_read).await?;
    let mut touched = HashSet::new();
    for mid in &changed {
        for cid in conversations::find_conversation_ids_by_message(&mut tx, &cmd.account_id, mid)
            .await?
        {
            touched.insert(cid.into_inner());
        }
    }
    for cid in touched {
        conversations::recompute_projection(
            &mut tx,
            &cmd.account_id,
            &crate::domain::value_objects::ConversationId::new(cid),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}
