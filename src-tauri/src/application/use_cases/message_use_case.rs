//! 邮件用例
use crate::infrastructure::repository::messages;
use sqlx::SqlitePool;

use crate::application::dto::MarkMessagesReadCommand;
use crate::application::error::AppError;
use crate::domain::entities::MessageWithRecipients;
use crate::domain::value_objects::{AccountId, MessageId};

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

/// 确保邮件正文已加载，必要时从 Outlook 获取并回写本地。
///
/// 正文为空字符串表示已加载，不能按未加载重复获取；网络请求应在写事务外执行。
/// 当前为骨架，返回未实现错误。
pub async fn ensure_message_body(
    db: &SqlitePool,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<MessageWithRecipients, AppError> {
    let _ = (db, account_id, message_id);
    Err(AppError::not_implemented(
        "message_use_case::ensure_message_body",
    ))
}

/// 批量修改指定账户邮件的已读状态，并记录待推送到 Outlook 的本地变更。
///
/// 邮件状态、待提交操作及关联会话未读数须原子更新；本地成功不代表远端已确认。
/// 当前为骨架，返回未实现错误。
pub async fn mark_read(db: &SqlitePool, cmd: MarkMessagesReadCommand) -> Result<(), AppError> {
    let _ = (db, cmd);
    Err(AppError::not_implemented("message_use_case::mark_read"))
}
