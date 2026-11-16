//! 群组用例
use crate::infrastructure::repository::{groups, sessions};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::application::dto::{
    CreateManualGroupCommand, CursorPage, GroupListQuery, MergeSessionsCommand,
    ReassignMessagesToGroupCommand, RenameGroupCommand, UpdateGroupSendMembersCommand,
};
use crate::application::error::AppError;
use crate::domain::entities::{Group, GroupParticipant};
use crate::domain::value_objects::{AccountId, GroupId, GroupSource, TimestampMs};
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

    let session = sessions::new_group(cmd.account_id.clone(), group.id.clone(), now);
    sessions::upsert(&mut tx, &session)
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
    group.name = name.to_string();
    group.name_is_manual = true;
    group.updated_at = now_ms();
    groups::update(&mut tx, &group)
        .await
        .map_err(AppError::from)?;
    tx.commit().await?;
    Ok(group)
}

/// 合并两个群聊 Session（按 group 找到各自 Session 后合并）
pub async fn merge_group_sessions(
    db: &SqlitePool,
    cmd: MergeSessionsCommand,
) -> Result<crate::domain::entities::Session, AppError> {
    crate::application::use_cases::session_use_case::merge_sessions(
        db,
        &cmd.account_id,
        &cmd.source_session_id,
        &cmd.target_session_id,
    )
    .await
}

/// 将选中消息改挂到目标群的存活 Session
pub async fn reassign_messages_to_group(
    db: &SqlitePool,
    cmd: ReassignMessagesToGroupCommand,
) -> Result<(), AppError> {
    if cmd.message_ids.is_empty() {
        return Ok(());
    }
    let mut tx = db.begin().await?;
    let _group = groups::get(&mut tx, &cmd.account_id, &cmd.target_group_id)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::not_found(format!("目标群组不存在: {}", cmd.target_group_id)))?;

    let now = now_ms();
    let target = if let Some(s) =
        sessions::get_group_by_group_id(&mut tx, &cmd.account_id, &cmd.target_group_id)
            .await
            .map_err(AppError::from)?
    {
        if s.merged_into_id.is_some() {
            sessions::resolve_root(&mut tx, &cmd.account_id, &s.id)
                .await
                .map_err(AppError::from)?
                .ok_or_else(|| AppError::not_found("目标群聊 Session 不存在"))?
        } else {
            s
        }
    } else {
        let s = sessions::new_group(cmd.account_id.clone(), cmd.target_group_id.clone(), now);
        sessions::upsert(&mut tx, &s)
            .await
            .map_err(AppError::from)?;
        s
    };

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
    tx.commit().await?;
    Ok(())
}

pub async fn update_send_members(
    db: &SqlitePool,
    cmd: UpdateGroupSendMembersCommand,
) -> Result<Vec<GroupParticipant>, AppError> {
    let mut tx = db.begin().await?;
    let group = groups::get(&mut tx, &cmd.account_id, &cmd.group_id)
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
