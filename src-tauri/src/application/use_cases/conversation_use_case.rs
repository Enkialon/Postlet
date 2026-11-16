//! 会话用例（主导航）
use crate::infrastructure::repository::{contacts, conversations, groups};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::application::dto::{
    ConversationListItem, ConversationListQuery, ConversationMessagesQuery, CursorPage,
    EnsureContactConversationCommand, EnsureGroupConversationCommand,
};
use crate::application::error::AppError;
use crate::domain::entities::{Contact, Conversation, MessageWithRecipients};
use crate::domain::value_objects::{
    AccountId, ContactId, ConversationId, ConversationKind, TimestampMs,
};
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ms() -> TimestampMs {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub async fn list_conversations(
    db: &SqlitePool,
    query: ConversationListQuery,
) -> Result<CursorPage<ConversationListItem>, AppError> {
    let mut conn = db.acquire().await?;
    conversations::list(&mut conn, &query)
        .await
        .map_err(Into::into)
}

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
pub async fn ensure_contact_conversation(
    db: &SqlitePool,
    cmd: EnsureContactConversationCommand,
) -> Result<Conversation, AppError> {
    let mut tx = db.begin().await?;
    if let Some(mut existing) =
        conversations::get_contact_conversation(&mut tx, &cmd.account_id, &cmd.normalized_address)
            .await
            .map_err(AppError::from)?
    {
        // 从通讯录显式打开 → 取消隐藏
        if existing.hidden_at.is_some() {
            let now = now_ms();
            conversations::set_hidden_at(
                &mut tx,
                &cmd.account_id,
                &existing.id,
                None,
                now,
            )
            .await
            .map_err(AppError::from)?;
            existing.hidden_at = None;
            existing.updated_at = now;
        }
        tx.commit().await?;
        return Ok(existing);
    }

    let now = now_ms();
    let contact = if let Some(c) =
        contacts::get_by_address(&mut tx, &cmd.account_id, &cmd.normalized_address)
            .await
            .map_err(AppError::from)?
    {
        c
    } else {
        let c = Contact {
            id: ContactId::new(Uuid::new_v4().to_string()),
            account_id: cmd.account_id.clone(),
            graph_id: None,
            display_name: cmd
                .display_name
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| cmd.normalized_address.as_str().to_string()),
            normalized_address: cmd.normalized_address.clone(),
            created_at: now,
            updated_at: now,
        };
        contacts::upsert(&mut tx, &c)
            .await
            .map_err(AppError::from)?;
        c
    };

    let conversation = Conversation {
        id: ConversationId::new(format!("conv-{}", Uuid::new_v4())),
        account_id: cmd.account_id.clone(),
        kind: ConversationKind::Contact,
        normalized_address: Some(cmd.normalized_address),
        contact_id: Some(contact.id),
        group_id: None,
        last_message_at: None,
        last_message_id: None,
        unread_count: 0,
        hidden_at: None,
        created_at: now,
        updated_at: now,
    };
    conversations::upsert(&mut tx, &conversation)
        .await
        .map_err(AppError::from)?;
    tx.commit().await?;
    Ok(conversation)
}

/// 确保存在群组会话（解析到存活根群组）。
pub async fn ensure_group_conversation(
    db: &SqlitePool,
    cmd: EnsureGroupConversationCommand,
) -> Result<Conversation, AppError> {
    let mut tx = db.begin().await?;
    let root = groups::resolve_root(&mut tx, &cmd.account_id, &cmd.group_id)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::not_found(format!("群组不存在: {}", cmd.group_id)))?;

    if let Some(mut existing) =
        conversations::get_group_conversation(&mut tx, &cmd.account_id, &root.id)
            .await
            .map_err(AppError::from)?
    {
        if existing.hidden_at.is_some() {
            let now = now_ms();
            conversations::set_hidden_at(&mut tx, &cmd.account_id, &existing.id, None, now)
                .await
                .map_err(AppError::from)?;
            existing.hidden_at = None;
            existing.updated_at = now;
        }
        tx.commit().await?;
        return Ok(existing);
    }

    let now = now_ms();
    let conversation = Conversation {
        id: ConversationId::new(format!("conv-{}", Uuid::new_v4())),
        account_id: cmd.account_id.clone(),
        kind: ConversationKind::Group,
        normalized_address: None,
        contact_id: None,
        group_id: Some(root.id.clone()),
        last_message_at: None,
        last_message_id: None,
        unread_count: 0,
        hidden_at: None,
        created_at: now,
        updated_at: now,
    };
    conversations::upsert(&mut tx, &conversation)
        .await
        .map_err(AppError::from)?;
    tx.commit().await?;
    Ok(conversation)
}

/// 本地隐藏会话（不删邮件）；有更新入站/发信后投影会自动取消隐藏。
pub async fn hide_conversation(
    db: &SqlitePool,
    account_id: &AccountId,
    conversation_id: &ConversationId,
) -> Result<(), AppError> {
    let mut conn = db.acquire().await?;
    let now = now_ms();
    conversations::set_hidden_at(&mut conn, account_id, conversation_id, Some(now), now)
        .await
        .map_err(Into::into)
}
