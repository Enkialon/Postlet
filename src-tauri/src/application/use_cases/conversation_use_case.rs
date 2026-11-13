//! 会话用例（主导航）
use crate::infrastructure::repository::conversations;
use sqlx::SqlitePool;

use crate::application::dto::{
    ConversationListItem, ConversationListQuery, ConversationMessagesQuery, CursorPage,
    EnsureContactConversationCommand, EnsureGroupConversationCommand,
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

/// 在指定账户内获取会话详情（含列表预览字段）；不存在时返回未找到错误。
pub async fn get_conversation(
    db: &SqlitePool,
    account_id: &AccountId,
    conversation_id: &ConversationId,
) -> Result<ConversationListItem, AppError> {
    let mut conn = db.acquire().await?;
    conversations::get_list_item(&mut conn, account_id, conversation_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("会话不存在: {conversation_id}")))
}

/// 按游标及方向分页读取会话附着的邮件和收件人，不按文件夹组织邮件。
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
/// 当前为骨架，返回未实现错误。
pub async fn ensure_contact_conversation(
    db: &SqlitePool,
    cmd: EnsureContactConversationCommand,
) -> Result<Conversation, AppError> {
    let _ = (db, cmd);
    Err(AppError::not_implemented(
        "conversation_use_case::ensure_contact_conversation",
    ))
}

/// 确保存在群组会话（解析到存活根群组）。
///
/// 当前为骨架，返回未实现错误。
pub async fn ensure_group_conversation(
    db: &SqlitePool,
    cmd: EnsureGroupConversationCommand,
) -> Result<Conversation, AppError> {
    let _ = (db, cmd);
    Err(AppError::not_implemented(
        "conversation_use_case::ensure_group_conversation",
    ))
}
