//! 会话用例（主导航）
use crate::infrastructure::repository::{contacts, groups, sessions};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::application::dto::{
    CursorPage, EnsureGroupSessionCommand, EnsurePrivateSessionCommand, SessionListItem,
    SessionListQuery, SessionMessagesQuery,
};
use crate::application::error::AppError;
use crate::domain::entities::{Contact, MessageWithRecipients, Session};
use crate::domain::value_objects::{AccountId, ContactId, SessionId, TimestampMs};
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ms() -> TimestampMs {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub async fn list_sessions(
    db: &SqlitePool,
    query: SessionListQuery,
) -> Result<CursorPage<SessionListItem>, AppError> {
    let mut conn = db.acquire().await?;
    sessions::list(&mut conn, &query)
        .await
        .map_err(Into::into)
}

pub async fn get_session(
    db: &SqlitePool,
    account_id: &AccountId,
    session_id: &SessionId,
) -> Result<SessionListItem, AppError> {
    let mut conn = db.acquire().await?;
    sessions::get_list_item(&mut conn, account_id, session_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("会话不存在: {session_id}")))
}

pub async fn list_messages(
    db: &SqlitePool,
    query: SessionMessagesQuery,
) -> Result<CursorPage<MessageWithRecipients>, AppError> {
    let mut conn = db.acquire().await?;
    sessions::list_messages(&mut conn, &query)
        .await
        .map_err(Into::into)
}

/// 按账户及规范化地址取得或创建私聊 Session，必要时创建占位联系人。
pub async fn ensure_private_session(
    db: &SqlitePool,
    cmd: EnsurePrivateSessionCommand,
) -> Result<Session, AppError> {
    let mut tx = db.begin().await?;
    if let Some(mut existing) =
        sessions::get_private_by_address(&mut tx, &cmd.account_id, &cmd.normalized_address)
            .await
            .map_err(AppError::from)?
    {
        if existing.hidden_at.is_some() {
            let now = now_ms();
            sessions::set_hidden_at(&mut tx, &cmd.account_id, &existing.id, None, now)
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

    let session = sessions::new_private(
        cmd.account_id.clone(),
        cmd.normalized_address,
        Some(contact.id),
        now,
    );
    sessions::upsert(&mut tx, &session)
        .await
        .map_err(AppError::from)?;
    tx.commit().await?;
    Ok(session)
}

/// 确保存在群聊 Session。
pub async fn ensure_group_session(
    db: &SqlitePool,
    cmd: EnsureGroupSessionCommand,
) -> Result<Session, AppError> {
    let mut tx = db.begin().await?;
    let group = groups::get(&mut tx, &cmd.account_id, &cmd.group_id)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::not_found(format!("群组不存在: {}", cmd.group_id)))?;

    if let Some(mut existing) =
        sessions::get_group_by_group_id(&mut tx, &cmd.account_id, &group.id)
            .await
            .map_err(AppError::from)?
    {
        // 若已合并，返回根会话
        if existing.merged_into_id.is_some() {
            let root = sessions::resolve_root(&mut tx, &cmd.account_id, &existing.id)
                .await
                .map_err(AppError::from)?
                .ok_or_else(|| AppError::not_found("合并根会话不存在"))?;
            tx.commit().await?;
            return Ok(root);
        }
        if existing.hidden_at.is_some() {
            let now = now_ms();
            sessions::set_hidden_at(&mut tx, &cmd.account_id, &existing.id, None, now)
                .await
                .map_err(AppError::from)?;
            existing.hidden_at = None;
            existing.updated_at = now;
        }
        tx.commit().await?;
        return Ok(existing);
    }

    let now = now_ms();
    let session = sessions::new_group(cmd.account_id.clone(), group.id, now);
    sessions::upsert(&mut tx, &session)
        .await
        .map_err(AppError::from)?;
    tx.commit().await?;
    Ok(session)
}

/// 本地隐藏会话（不删邮件）；有更新后投影会自动取消隐藏。
pub async fn hide_session(
    db: &SqlitePool,
    account_id: &AccountId,
    session_id: &SessionId,
) -> Result<(), AppError> {
    let mut conn = db.acquire().await?;
    let now = now_ms();
    sessions::set_hidden_at(&mut conn, account_id, session_id, Some(now), now)
        .await
        .map_err(Into::into)
}

/// 合并两个 Session：消息迁入 target，source.merged_into_id = target。
pub async fn merge_sessions(
    db: &SqlitePool,
    account_id: &AccountId,
    source_session_id: &SessionId,
    target_session_id: &SessionId,
) -> Result<Session, AppError> {
    if source_session_id == target_session_id {
        return Err(AppError::validation("不能合并到自身"));
    }
    let mut tx = db.begin().await?;
    let target = sessions::resolve_root(&mut tx, account_id, target_session_id)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::not_found(format!("目标会话不存在: {target_session_id}")))?;
    let source = sessions::get(&mut tx, account_id, source_session_id)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::not_found(format!("源会话不存在: {source_session_id}")))?;
    if source.merged_into_id.is_some() {
        return Err(AppError::conflict("源会话已合并"));
    }
    if target.id == source.id {
        return Err(AppError::validation("源与目标解析为同一根会话"));
    }

    let now = now_ms();
    sessions::move_all_messages(&mut tx, account_id, &source.id, &target.id)
        .await
        .map_err(AppError::from)?;

    // 群聊合并：并集迁移成员
    if let (Some(src_gid), Some(dst_gid)) = (&source.group_id, &target.group_id) {
        let source_parts = groups::list_participants(&mut tx, account_id, src_gid)
            .await
            .map_err(AppError::from)?;
        let mut migrated = Vec::new();
        for mut p in source_parts {
            p.group_id = dst_gid.clone();
            migrated.push(p);
        }
        groups::upsert_participants(&mut tx, &migrated)
            .await
            .map_err(AppError::from)?;
    }

    sessions::merge_into(&mut tx, account_id, &source.id, &target.id, now)
        .await
        .map_err(AppError::from)?;
    sessions::recompute_projection(&mut tx, account_id, &target.id)
        .await
        .map_err(AppError::from)?;
    let out = sessions::get(&mut tx, account_id, &target.id)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::not_found("目标会话不存在"))?;
    tx.commit().await?;
    Ok(out)
}
