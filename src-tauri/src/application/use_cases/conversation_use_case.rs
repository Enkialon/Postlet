//! 会话用例（主导航）
use crate::infrastructure::repository::conversations;
use sqlx::SqlitePool;

use crate::application::dto::{
    ConversationListItem, ConversationListQuery, ConversationMessagesQuery, CursorPage,
    EnsureContactConversationCommand,
};
use crate::application::error::AppError;
use crate::domain::entities::{Conversation, MessageWithRecipients};
use crate::domain::value_objects::{AccountId, ConversationId};

/// 分页读取账户内的联系人或群组会话，供主导航列表展示。
///
/// 未读数和最新消息信息以本地会话投影为准，不在应用层重新聚合。
pub async fn list_conversations(
    db: &SqlitePool,
    query: ConversationListQuery,
) -> Result<CursorPage<ConversationListItem>, AppError> {
    let mut conn = db.acquire().await?;
    conversations::list(&mut conn, &query)
        .await
        .map_err(Into::into)
}

/// 在指定账户内获取会话详情；不存在时返回未找到错误。
///
/// 当前仅返回会话实体，最新邮件预览、联系人名称和群组名称尚未补全。
pub async fn get_conversation(
    db: &SqlitePool,
    account_id: &AccountId,
    conversation_id: &ConversationId,
) -> Result<ConversationListItem, AppError> {
    let mut conn = db.acquire().await?;
    // 读模型补全由仓储实现，当前保持已有实体返回行为
    let conversation = conversations::get(&mut conn, account_id, conversation_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("会话不存在: {conversation_id}")))?;
    Ok(ConversationListItem {
        conversation,
        last_message_preview: None,
        contact_display_name: None,
        group_name: None,
    })
}

/// 按游标及方向分页读取会话附着的邮件和收件人，不按文件夹组织邮件。
///
/// 当前仓储查询尚未实现，调用会返回未实现错误。
pub async fn list_messages(
    db: &SqlitePool,
    query: ConversationMessagesQuery,
) -> Result<CursorPage<MessageWithRecipients>, AppError> {
    let mut conn = db.acquire().await?;
    conversations::list_messages(&mut conn, &query)
        .await
        .map_err(Into::into)
}

/// 按账户及规范化地址取得或创建联系人会话，必要时创建占位联系人。
///
/// 联系人与会话写入应共用事务；当前为骨架，返回未实现错误。
pub async fn ensure_contact_conversation(
    db: &SqlitePool,
    cmd: EnsureContactConversationCommand,
) -> Result<Conversation, AppError> {
    let _ = (db, cmd);
    Err(AppError::not_implemented(
        "conversation_use_case::ensure_contact_conversation",
    ))
}
