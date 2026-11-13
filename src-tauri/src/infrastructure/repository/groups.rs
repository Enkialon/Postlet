//! SQLite groups 模块函数；连接与事务由调用方持有。

use crate::application::dto::{CursorPage, GroupListQuery};
use crate::application::error::PortError;
use crate::domain::entities::{Group, GroupParticipant, GroupThread};
use crate::domain::value_objects::{AccountId, GroupId, ThreadId};
use sqlx::SqliteConnection;

pub async fn get(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    id: &GroupId,
) -> Result<Option<Group>, PortError> {
    let row = sqlx::query("SELECT * FROM groups WHERE account_id = ? AND id = ?")
        .bind(account_id.as_str())
        .bind(id.as_str())
        .fetch_optional(&mut *conn)
        .await?;
    row.as_ref()
        .map(super::rows::group)
        .transpose()
        .map_err(Into::into)
}

pub async fn resolve_root(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    id: &GroupId,
) -> Result<Option<Group>, PortError> {
    let _ = (conn, account_id, id);
    Err(PortError::Storage {
        message: "尚未实现: repository::groups::resolve_root".into(),
    })
}

pub async fn list(
    conn: &mut SqliteConnection,
    query: &GroupListQuery,
) -> Result<CursorPage<Group>, PortError> {
    if query.limit == 0 || query.limit > 500 {
        return Err(PortError::Conflict {
            message: "分页大小须为 1..500".into(),
        });
    }
    let rows = sqlx::query("SELECT * FROM groups WHERE account_id = ? AND (? IS NULL OR id > ?) AND (? OR merged_into_id IS NULL) ORDER BY id LIMIT ?")
        .bind(query.account_id.as_str()).bind(query.cursor.as_deref()).bind(query.cursor.as_deref()).bind(query.include_merged).bind(i64::from(query.limit) + 1).fetch_all(&mut *conn).await?;
    let has_more = rows.len() > query.limit as usize;
    let items = rows
        .iter()
        .take(query.limit as usize)
        .map(super::rows::group)
        .collect::<Result<Vec<_>, _>>()?;
    let next_cursor = if has_more {
        items.last().map(|x| x.id.as_str().to_owned())
    } else {
        None
    };
    Ok(CursorPage {
        items,
        next_cursor,
        has_more,
    })
}

pub async fn insert(conn: &mut SqliteConnection, group: &Group) -> Result<(), PortError> {
    let source = match group.source {
        crate::domain::value_objects::GroupSource::Auto => "auto",
        crate::domain::value_objects::GroupSource::Manual => "manual",
    };
    sqlx::query("INSERT INTO groups (id, account_id, name, name_is_manual, source, send_members_is_manual, merged_into_id, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(group.id.as_str()).bind(group.account_id.as_str()).bind(&group.name)
        .bind(group.name_is_manual).bind(source).bind(group.send_members_is_manual)
        .bind(group.merged_into_id.as_ref().map(|id| id.as_str()))
        .bind(group.created_at).bind(group.updated_at).execute(conn).await?;
    Ok(())
}

pub async fn update(conn: &mut SqliteConnection, group: &Group) -> Result<(), PortError> {
    let _ = (conn, group);
    Err(PortError::Storage {
        message: "尚未实现: repository::groups::update".into(),
    })
}

pub async fn list_participants(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    group_id: &GroupId,
) -> Result<Vec<GroupParticipant>, PortError> {
    sqlx::query("SELECT * FROM group_participants WHERE account_id = ? AND group_id = ? ORDER BY normalized_address")
        .bind(account_id.as_str()).bind(group_id.as_str()).fetch_all(&mut *conn).await?
        .iter().map(super::rows::groupparticipant).collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub async fn upsert_participants(
    conn: &mut SqliteConnection,
    participants: &[GroupParticipant],
) -> Result<(), PortError> {
    let _ = (conn, participants);
    Err(PortError::Storage {
        message: "尚未实现: repository::groups::upsert_participants".into(),
    })
}

pub async fn replace_send_members(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    group_id: &GroupId,
    members: &[GroupParticipant],
    send_members_is_manual: bool,
) -> Result<(), PortError> {
    let _ = (conn, account_id, group_id, members, send_members_is_manual);
    Err(PortError::Storage {
        message: "尚未实现: repository::groups::replace_send_members".into(),
    })
}

pub async fn get_thread_link(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    thread_id: &ThreadId,
) -> Result<Option<GroupThread>, PortError> {
    let _ = (conn, account_id, thread_id);
    Err(PortError::Storage {
        message: "尚未实现: repository::groups::get_thread_link".into(),
    })
}

pub async fn upsert_thread_link(
    conn: &mut SqliteConnection,
    link: &GroupThread,
) -> Result<(), PortError> {
    let _ = (conn, link);
    Err(PortError::Storage {
        message: "尚未实现: repository::groups::upsert_thread_link".into(),
    })
}

pub async fn list_threads_by_group(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    group_id: &GroupId,
) -> Result<Vec<GroupThread>, PortError> {
    let _ = (conn, account_id, group_id);
    Err(PortError::Storage {
        message: "尚未实现: repository::groups::list_threads_by_group".into(),
    })
}
