//! 群组用例
use crate::infrastructure::repository::{conversations, groups};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::application::dto::{
    CreateManualGroupCommand, CursorPage, GroupListQuery, MergeGroupsCommand,
    MoveThreadToGroupCommand, RenameGroupCommand, UpdateGroupSendMembersCommand,
};
use crate::application::error::AppError;
use crate::domain::entities::{Conversation, Group, GroupParticipant, GroupThread};
use crate::domain::value_objects::{
    AccountId, ConversationId, ConversationKind, GroupId, GroupSource, TimestampMs,
};
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ms() -> TimestampMs {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub async fn list_groups(
    db: &SqlitePool,
    query: GroupListQuery,
) -> Result<CursorPage<Group>, AppError> {
    let mut conn = db.acquire().await?;
    groups::list(&mut conn, &query).await.map_err(Into::into)
}

pub async fn get_group(
    db: &SqlitePool,
    account_id: &AccountId,
    group_id: &GroupId,
) -> Result<Group, AppError> {
    let mut conn = db.acquire().await?;
    groups::get(&mut conn, account_id, group_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("群组不存在: {group_id}")))
}

pub async fn list_participants(
    db: &SqlitePool,
    account_id: &AccountId,
    group_id: &GroupId,
) -> Result<Vec<GroupParticipant>, AppError> {
    let mut conn = db.acquire().await?;
    groups::list_participants(&mut conn, account_id, group_id)
        .await
        .map_err(Into::into)
}

pub async fn create_manual_group(
    db: &SqlitePool,
    cmd: CreateManualGroupCommand,
) -> Result<Group, AppError> {
    let name = cmd.name.trim();
    if name.is_empty() {
        return Err(AppError::validation("群组名称不能为空"));
    }
    let now = now_ms();
    let group = Group {
        id: GroupId::new(Uuid::new_v4().to_string()),
        account_id: cmd.account_id.clone(),
        name: name.to_string(),
        name_is_manual: true,
        source: GroupSource::Manual,
        send_members_is_manual: true,
        merged_into_id: None,
        created_at: now,
        updated_at: now,
    };

    let mut participants = Vec::new();
    let mut seen = HashSet::new();
    for addr in cmd.send_member_addresses {
        if !seen.insert(addr.as_str().to_string()) {
            continue;
        }
        participants.push(GroupParticipant {
            account_id: cmd.account_id.clone(),
            group_id: group.id.clone(),
            normalized_address: addr.clone(),
            display_name: addr.as_str().to_string(),
            is_observed: false,
            is_send_member: true,
            first_seen_at: now,
            last_seen_at: now,
        });
    }

    let mut tx = db.begin().await?;
    groups::insert(&mut tx, &group)
        .await
        .map_err(AppError::from)?;
    groups::upsert_participants(&mut tx, &participants)
        .await
        .map_err(AppError::from)?;

    let conversation = Conversation {
        id: ConversationId::new(format!("conv-{}", Uuid::new_v4())),
        account_id: cmd.account_id.clone(),
        kind: ConversationKind::Group,
        normalized_address: None,
        contact_id: None,
        group_id: Some(group.id.clone()),
        last_message_at: None,
        last_message_id: None,
        unread_count: 0,
        created_at: now,
        updated_at: now,
    };
    conversations::upsert(&mut tx, &conversation)
        .await
        .map_err(AppError::from)?;
    tx.commit().await?;
    Ok(group)
}

pub async fn rename_group(db: &SqlitePool, cmd: RenameGroupCommand) -> Result<Group, AppError> {
    let name = cmd.name.trim();
    if name.is_empty() {
        return Err(AppError::validation("群组名称不能为空"));
    }
    let mut tx = db.begin().await?;
    let mut group = groups::get(&mut tx, &cmd.account_id, &cmd.group_id)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::not_found(format!("群组不存在: {}", cmd.group_id)))?;
    if group.merged_into_id.is_some() {
        return Err(AppError::conflict("不能重命名已合并的群组"));
    }
    group.name = name.to_string();
    group.name_is_manual = true;
    group.updated_at = now_ms();
    groups::update(&mut tx, &group)
        .await
        .map_err(AppError::from)?;
    tx.commit().await?;
    Ok(group)
}

pub async fn merge_groups(db: &SqlitePool, cmd: MergeGroupsCommand) -> Result<Group, AppError> {
    if cmd.source_group_id == cmd.target_group_id {
        return Err(AppError::validation("不能合并到自身"));
    }
    let mut tx = db.begin().await?;
    let target = groups::resolve_root(&mut tx, &cmd.account_id, &cmd.target_group_id)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::not_found(format!("目标群组不存在: {}", cmd.target_group_id)))?;
    let mut source = groups::get(&mut tx, &cmd.account_id, &cmd.source_group_id)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::not_found(format!("源群组不存在: {}", cmd.source_group_id)))?;
    if source.merged_into_id.is_some() {
        return Err(AppError::conflict("源群组已合并"));
    }
    if target.id == source.id {
        return Err(AppError::validation("源与目标解析为同一根群组"));
    }

    // 迁移线程归属
    let threads = groups::list_threads_by_group(&mut tx, &cmd.account_id, &source.id)
        .await
        .map_err(AppError::from)?;
    let now = now_ms();
    for mut link in threads {
        link.group_id = target.id.clone();
        link.linked_at = now;
        groups::upsert_thread_link(&mut tx, &link)
            .await
            .map_err(AppError::from)?;
    }

    // 迁移成员（观测/发信并集）
    let source_parts = groups::list_participants(&mut tx, &cmd.account_id, &source.id)
        .await
        .map_err(AppError::from)?;
    let mut migrated = Vec::new();
    for mut p in source_parts {
        p.group_id = target.id.clone();
        migrated.push(p);
    }
    groups::upsert_participants(&mut tx, &migrated)
        .await
        .map_err(AppError::from)?;

    // 迁移群组会话消息附着
    if let Some(src_conv) =
        conversations::get_group_conversation(&mut tx, &cmd.account_id, &source.id)
            .await
            .map_err(AppError::from)?
    {
        let dst_conv = if let Some(c) =
            conversations::get_group_conversation(&mut tx, &cmd.account_id, &target.id)
                .await
                .map_err(AppError::from)?
        {
            c
        } else {
            let c = Conversation {
                id: ConversationId::new(format!("conv-{}", Uuid::new_v4())),
                account_id: cmd.account_id.clone(),
                kind: ConversationKind::Group,
                normalized_address: None,
                contact_id: None,
                group_id: Some(target.id.clone()),
                last_message_at: None,
                last_message_id: None,
                unread_count: 0,
                created_at: now,
                updated_at: now,
            };
            conversations::upsert(&mut tx, &c)
                .await
                .map_err(AppError::from)?;
            c
        };
        // 把源会话的 message 链接搬到目标
        sqlx::query(
            "INSERT OR IGNORE INTO conversation_messages (account_id, conversation_id, message_id, attached_at)
             SELECT account_id, ?, message_id, attached_at FROM conversation_messages
             WHERE account_id = ? AND conversation_id = ?",
        )
        .bind(dst_conv.id.as_str())
        .bind(cmd.account_id.as_str())
        .bind(src_conv.id.as_str())
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
        sqlx::query(
            "DELETE FROM conversation_messages WHERE account_id = ? AND conversation_id = ?",
        )
        .bind(cmd.account_id.as_str())
        .bind(src_conv.id.as_str())
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
        conversations::recompute_projection(&mut tx, &cmd.account_id, &dst_conv.id)
            .await
            .map_err(AppError::from)?;
    }

    source.merged_into_id = Some(target.id.clone());
    source.updated_at = now;
    groups::update(&mut tx, &source)
        .await
        .map_err(AppError::from)?;
    tx.commit().await?;
    Ok(target)
}

pub async fn move_thread_to_group(
    db: &SqlitePool,
    cmd: MoveThreadToGroupCommand,
) -> Result<(), AppError> {
    let mut tx = db.begin().await?;
    let target = groups::resolve_root(&mut tx, &cmd.account_id, &cmd.target_group_id)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::not_found(format!("目标群组不存在: {}", cmd.target_group_id)))?;

    let now = now_ms();
    let old = groups::get_thread_link(&mut tx, &cmd.account_id, &cmd.thread_id)
        .await
        .map_err(AppError::from)?;
    groups::upsert_thread_link(
        &mut tx,
        &GroupThread {
            account_id: cmd.account_id.clone(),
            group_id: target.id.clone(),
            thread_id: cmd.thread_id.clone(),
            linked_at: now,
        },
    )
    .await
    .map_err(AppError::from)?;

    // 确保目标群组会话存在
    let dst_conv = if let Some(c) =
        conversations::get_group_conversation(&mut tx, &cmd.account_id, &target.id)
            .await
            .map_err(AppError::from)?
    {
        c
    } else {
        let c = Conversation {
            id: ConversationId::new(format!("conv-{}", Uuid::new_v4())),
            account_id: cmd.account_id.clone(),
            kind: ConversationKind::Group,
            normalized_address: None,
            contact_id: None,
            group_id: Some(target.id.clone()),
            last_message_at: None,
            last_message_id: None,
            unread_count: 0,
            created_at: now,
            updated_at: now,
        };
        conversations::upsert(&mut tx, &c)
            .await
            .map_err(AppError::from)?;
        c
    };

    // 将该 thread 下消息附着到目标群组会话
    sqlx::query(
        "INSERT OR IGNORE INTO conversation_messages (account_id, conversation_id, message_id, attached_at)
         SELECT m.account_id, ?, m.id, ?
         FROM messages m
         WHERE m.account_id = ? AND m.thread_id = ?",
    )
    .bind(dst_conv.id.as_str())
    .bind(now)
    .bind(cmd.account_id.as_str())
    .bind(cmd.thread_id.as_str())
    .execute(&mut *tx)
    .await
    .map_err(AppError::from)?;

    if let Some(old_link) = old {
        if old_link.group_id != target.id {
            if let Some(src_conv) =
                conversations::get_group_conversation(&mut tx, &cmd.account_id, &old_link.group_id)
                    .await
                    .map_err(AppError::from)?
            {
                sqlx::query(
                    "DELETE FROM conversation_messages
                     WHERE account_id = ? AND conversation_id = ?
                       AND message_id IN (
                         SELECT id FROM messages WHERE account_id = ? AND thread_id = ?
                       )",
                )
                .bind(cmd.account_id.as_str())
                .bind(src_conv.id.as_str())
                .bind(cmd.account_id.as_str())
                .bind(cmd.thread_id.as_str())
                .execute(&mut *tx)
                .await
                .map_err(AppError::from)?;
                conversations::recompute_projection(&mut tx, &cmd.account_id, &src_conv.id)
                    .await
                    .map_err(AppError::from)?;
            }
        }
    }

    conversations::recompute_projection(&mut tx, &cmd.account_id, &dst_conv.id)
        .await
        .map_err(AppError::from)?;
    tx.commit().await?;
    Ok(())
}

pub async fn update_send_members(
    db: &SqlitePool,
    cmd: UpdateGroupSendMembersCommand,
) -> Result<Vec<GroupParticipant>, AppError> {
    let mut tx = db.begin().await?;
    let group = groups::resolve_root(&mut tx, &cmd.account_id, &cmd.group_id)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::not_found(format!("群组不存在: {}", cmd.group_id)))?;

    let now = now_ms();
    let mut members = Vec::new();
    let mut seen = HashSet::new();
    for addr in cmd.send_member_addresses {
        if !seen.insert(addr.as_str().to_string()) {
            continue;
        }
        members.push(GroupParticipant {
            account_id: cmd.account_id.clone(),
            group_id: group.id.clone(),
            normalized_address: addr.clone(),
            display_name: addr.as_str().to_string(),
            is_observed: false,
            is_send_member: true,
            first_seen_at: now,
            last_seen_at: now,
        });
    }
    groups::replace_send_members(
        &mut tx,
        &cmd.account_id,
        &group.id,
        &members,
        cmd.is_manual,
    )
    .await
    .map_err(AppError::from)?;
    let out = groups::list_participants(&mut tx, &cmd.account_id, &group.id)
        .await
        .map_err(AppError::from)?;
    tx.commit().await?;
    Ok(out)
}
