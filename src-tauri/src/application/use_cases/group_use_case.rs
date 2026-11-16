//! 群组用例
use crate::infrastructure::repository::{groups, sessions};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::application::dto::{
    CreateManualGroupCommand, CursorPage, GroupListQuery, HideGroupCommand, MergeSessionsCommand,
    RenameGroupCommand, UpdateGroupSendMembersCommand,
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
    groups::get_for_display(&mut conn, account_id, group_id)
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

/// 隐藏群组：只隐藏对应群聊 Session，不删邮件、不删群行。
pub async fn hide_group(db: &SqlitePool, cmd: HideGroupCommand) -> Result<(), AppError> {
    let mut tx = db.begin().await?;
    let _group = groups::get(&mut tx, &cmd.account_id, &cmd.group_id)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::not_found(format!("群组不存在: {}", cmd.group_id)))?;

    if let Some(session) =
        sessions::get_group_by_group_id(&mut tx, &cmd.account_id, &cmd.group_id)
            .await
            .map_err(AppError::from)?
    {
        let root = if session.merged_into_id.is_some() {
            sessions::resolve_root(&mut tx, &cmd.account_id, &session.id)
                .await
                .map_err(AppError::from)?
                .unwrap_or(session)
        } else {
            session
        };
        let now = now_ms();
        sessions::set_hidden_at(&mut tx, &cmd.account_id, &root.id, Some(now), now)
            .await
            .map_err(AppError::from)?;
    }
    tx.commit().await?;
    Ok(())
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
