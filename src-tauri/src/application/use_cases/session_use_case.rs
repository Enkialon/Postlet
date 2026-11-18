//! 会话用例（主导航）
use crate::infrastructure::repository::{accounts, contacts, groups, sessions};
use crate::query_page::CursorPage;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::application::dto::{
    EnsureGroupSessionCommand, EnsurePrivateSessionCommand, ReassignMessagesCommand,
    SessionListItem, SessionListQuery, SessionMessagesQuery,
};
use crate::application::error::AppError;
use crate::application::self_view::{apply_outbound_to_bundle, apply_outbound_to_preview};
use crate::domain::entities::{Contact, MessageWithRecipients, Session};
use crate::domain::services::resolve_self_addresses;
use crate::domain::value_objects::{AccountId, ContactId, SelfAddressSet, SessionId, TimestampMs};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ms() -> TimestampMs {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

async fn self_set_for(
    conn: &mut sqlx::SqliteConnection,
    account_id: &AccountId,
) -> Result<SelfAddressSet, AppError> {
    let account = accounts::get(conn, account_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("账户不存在: {account_id}")))?;
    Ok(resolve_self_addresses(&account))
}

async fn self_sets_for_accounts(
    conn: &mut sqlx::SqliteConnection,
    account_ids: impl IntoIterator<Item = AccountId>,
) -> Result<HashMap<AccountId, SelfAddressSet>, AppError> {
    let mut map = HashMap::new();
    for id in account_ids {
        if map.contains_key(&id) {
            continue;
        }
        let set = self_set_for(conn, &id).await?;
        map.insert(id, set);
    }
    Ok(map)
}

fn apply_session_item_outbound(item: &mut SessionListItem, self_set: &SelfAddressSet) {
    if let Some(ref mut preview) = item.last_message_preview {
        apply_outbound_to_preview(preview, self_set);
    }
}

pub async fn list_sessions(
    db: &SqlitePool,
    query: SessionListQuery,
) -> Result<CursorPage<SessionListItem>, AppError> {
    let mut conn = db.acquire().await?;
    let page = sessions::list(
        &mut conn,
        query.account_id.as_ref(),
        query.cursor.as_deref(),
        query.limit,
        query.kind,
        query.include_excluded_folders,
    )
    .await
    .map_err(AppError::from)?;
    let mut page = CursorPage {
        items: page
            .items
            .into_iter()
            .map(SessionListItem::from)
            .collect::<Vec<_>>(),
        next_cursor: page.next_cursor,
        has_more: page.has_more,
    };
    let account_ids = page
        .items
        .iter()
        .map(|i| i.session.account_id.clone())
        .collect::<Vec<_>>();
    let sets = self_sets_for_accounts(&mut conn, account_ids).await?;
    for item in &mut page.items {
        if let Some(set) = sets.get(&item.session.account_id) {
            apply_session_item_outbound(item, set);
        }
    }
    Ok(page)
}

pub async fn get_session(
    db: &SqlitePool,
    account_id: &AccountId,
    session_id: &SessionId,
) -> Result<SessionListItem, AppError> {
    let mut conn = db.acquire().await?;
    let projection = sessions::get_projection(&mut conn, account_id, session_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("会话不存在: {session_id}")))?;
    let mut item = SessionListItem::from(projection);
    let self_set = self_set_for(&mut conn, account_id).await?;
    apply_session_item_outbound(&mut item, &self_set);
    Ok(item)
}

pub async fn list_messages(
    db: &SqlitePool,
    query: SessionMessagesQuery,
) -> Result<CursorPage<MessageWithRecipients>, AppError> {
    let mut conn = db.acquire().await?;
    let mut page = sessions::list_messages(
        &mut conn,
        &query.account_id,
        &query.session_id,
        query.cursor.as_deref(),
        query.limit,
        query.direction,
    )
    .await
    .map_err(AppError::from)?;
    let self_set = self_set_for(&mut conn, &query.account_id).await?;
    for bundle in &mut page.items {
        apply_outbound_to_bundle(bundle, &self_set);
    }
    Ok(page)
}

/// 会话内尚无远端位置的发送记录；供发送状态区展示，不进入普通消息列表。
pub async fn list_pending_sends(
    db: &SqlitePool,
    account_id: &AccountId,
    session_id: &SessionId,
) -> Result<Vec<MessageWithRecipients>, AppError> {
    let mut conn = db.acquire().await?;
    let mut items = crate::infrastructure::repository::messages::list_pending_sends(&mut conn, account_id, session_id)
        .await
        .map_err(AppError::from)?;
    let self_set = self_set_for(&mut conn, account_id).await?;
    for bundle in &mut items {
        apply_outbound_to_bundle(bundle, &self_set);
    }
    Ok(items)
}

/// 按账户及规范化地址取得或创建私聊 Session；无档案时建 observed 短名档（非通讯录）。
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
            source: crate::domain::value_objects::ContactSource::Observed,
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

    if let Some(mut existing) = sessions::get_group_by_group_id(&mut tx, &cmd.account_id, &group.id)
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

/// 将选中消息改挂到目标存活 Session（私聊 / 群聊均可）。
pub async fn reassign_messages(
    db: &SqlitePool,
    cmd: ReassignMessagesCommand,
) -> Result<(), AppError> {
    if cmd.message_ids.is_empty() {
        return Ok(());
    }
    let mut tx = db.begin().await?;
    let target = sessions::resolve_root(&mut tx, &cmd.account_id, &cmd.target_session_id)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::not_found(format!("目标会话不存在: {}", cmd.target_session_id)))?;
    if target.merged_into_id.is_some() {
        return Err(AppError::conflict("目标会话已合并"));
    }

    let old_ids =
        sessions::reassign_messages(&mut tx, &cmd.account_id, &cmd.message_ids, &target.id)
            .await
            .map_err(AppError::from)?;
    for old in &old_ids {
        sessions::recompute_projection(&mut tx, &cmd.account_id, old)
            .await
            .map_err(AppError::from)?;
    }
    sessions::recompute_projection(&mut tx, &cmd.account_id, &target.id)
        .await
        .map_err(AppError::from)?;
    // 目标若被隐藏，迁入后重新露出
    if target.hidden_at.is_some() {
        let now = now_ms();
        sessions::set_hidden_at(&mut tx, &cmd.account_id, &target.id, None, now)
            .await
            .map_err(AppError::from)?;
    }
    tx.commit().await?;
    Ok(())
}
