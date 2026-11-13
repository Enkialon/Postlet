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
    let mut current = id.clone();
    for _ in 0..32 {
        let Some(group) = get(conn, account_id, &current).await? else {
            return Ok(None);
        };
        match group.merged_into_id {
            Some(next) => current = next,
            None => return Ok(Some(group)),
        }
    }
    Err(PortError::Conflict {
        message: "群组合并链过深或成环".into(),
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
    let rows = sqlx::query(
        "SELECT * FROM groups
         WHERE account_id = ?
           AND (? IS NULL OR id > ?)
           AND (? OR merged_into_id IS NULL)
         ORDER BY id LIMIT ?",
    )
    .bind(query.account_id.as_str())
    .bind(query.cursor.as_deref())
    .bind(query.cursor.as_deref())
    .bind(query.include_merged)
    .bind(i64::from(query.limit) + 1)
    .fetch_all(&mut *conn)
    .await?;
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
    sqlx::query(
        "INSERT INTO groups (
            id, account_id, name, name_is_manual, source, send_members_is_manual,
            merged_into_id, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(group.id.as_str())
    .bind(group.account_id.as_str())
    .bind(&group.name)
    .bind(group.name_is_manual)
    .bind(group.source.as_str())
    .bind(group.send_members_is_manual)
    .bind(group.merged_into_id.as_ref().map(|id| id.as_str()))
    .bind(group.created_at)
    .bind(group.updated_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn upsert(conn: &mut SqliteConnection, group: &Group) -> Result<(), PortError> {
    sqlx::query(
        "INSERT INTO groups (
            id, account_id, name, name_is_manual, source, send_members_is_manual,
            merged_into_id, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(account_id, id) DO UPDATE SET
            name=excluded.name,
            name_is_manual=excluded.name_is_manual,
            source=excluded.source,
            send_members_is_manual=excluded.send_members_is_manual,
            merged_into_id=excluded.merged_into_id,
            updated_at=excluded.updated_at",
    )
    .bind(group.id.as_str())
    .bind(group.account_id.as_str())
    .bind(&group.name)
    .bind(group.name_is_manual)
    .bind(group.source.as_str())
    .bind(group.send_members_is_manual)
    .bind(group.merged_into_id.as_ref().map(|id| id.as_str()))
    .bind(group.created_at)
    .bind(group.updated_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn update(conn: &mut SqliteConnection, group: &Group) -> Result<(), PortError> {
    let result = sqlx::query(
        "UPDATE groups SET
            name = ?, name_is_manual = ?, source = ?, send_members_is_manual = ?,
            merged_into_id = ?, updated_at = ?
         WHERE account_id = ? AND id = ?",
    )
    .bind(&group.name)
    .bind(group.name_is_manual)
    .bind(group.source.as_str())
    .bind(group.send_members_is_manual)
    .bind(group.merged_into_id.as_ref().map(|id| id.as_str()))
    .bind(group.updated_at)
    .bind(group.account_id.as_str())
    .bind(group.id.as_str())
    .execute(&mut *conn)
    .await?;
    if result.rows_affected() == 0 {
        return Err(PortError::Conflict {
            message: format!("群组不存在: {}", group.id),
        });
    }
    Ok(())
}

pub async fn list_participants(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    group_id: &GroupId,
) -> Result<Vec<GroupParticipant>, PortError> {
    sqlx::query(
        "SELECT * FROM group_participants WHERE account_id = ? AND group_id = ? ORDER BY normalized_address",
    )
    .bind(account_id.as_str())
    .bind(group_id.as_str())
    .fetch_all(&mut *conn)
    .await?
    .iter()
    .map(super::rows::groupparticipant)
    .collect::<Result<Vec<_>, _>>()
    .map_err(Into::into)
}

pub async fn upsert_participants(
    conn: &mut SqliteConnection,
    participants: &[GroupParticipant],
) -> Result<(), PortError> {
    for p in participants {
        sqlx::query(
            "INSERT INTO group_participants (
                account_id, group_id, normalized_address, display_name,
                is_observed, is_send_member, first_seen_at, last_seen_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(account_id, group_id, normalized_address) DO UPDATE SET
                display_name=excluded.display_name,
                is_observed=excluded.is_observed,
                is_send_member=excluded.is_send_member,
                first_seen_at=excluded.first_seen_at,
                last_seen_at=excluded.last_seen_at",
        )
        .bind(p.account_id.as_str())
        .bind(p.group_id.as_str())
        .bind(p.normalized_address.as_str())
        .bind(&p.display_name)
        .bind(p.is_observed)
        .bind(p.is_send_member)
        .bind(p.first_seen_at)
        .bind(p.last_seen_at)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

pub async fn replace_send_members(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    group_id: &GroupId,
    members: &[GroupParticipant],
    send_members_is_manual: bool,
) -> Result<(), PortError> {
    sqlx::query(
        "UPDATE group_participants SET is_send_member = 0
         WHERE account_id = ? AND group_id = ?",
    )
    .bind(account_id.as_str())
    .bind(group_id.as_str())
    .execute(&mut *conn)
    .await?;
    upsert_participants(conn, members).await?;
    sqlx::query(
        "UPDATE groups SET send_members_is_manual = ?, updated_at = ?
         WHERE account_id = ? AND id = ?",
    )
    .bind(send_members_is_manual)
    .bind(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0),
    )
    .bind(account_id.as_str())
    .bind(group_id.as_str())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn get_thread_link(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    thread_id: &ThreadId,
) -> Result<Option<GroupThread>, PortError> {
    let row = sqlx::query(
        "SELECT account_id, group_id, thread_id, linked_at FROM group_thread
         WHERE account_id = ? AND thread_id = ?",
    )
    .bind(account_id.as_str())
    .bind(thread_id.as_str())
    .fetch_optional(&mut *conn)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    use sqlx::Row;
    Ok(Some(GroupThread {
        account_id: AccountId::new(row.try_get::<String, _>("account_id")?),
        group_id: GroupId::new(row.try_get::<String, _>("group_id")?),
        thread_id: ThreadId::new(row.try_get::<String, _>("thread_id")?),
        linked_at: row.try_get("linked_at")?,
    }))
}

pub async fn upsert_thread_link(
    conn: &mut SqliteConnection,
    link: &GroupThread,
) -> Result<(), PortError> {
    sqlx::query(
        "INSERT INTO group_thread (account_id, group_id, thread_id, linked_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(account_id, thread_id) DO UPDATE SET
            group_id=excluded.group_id,
            linked_at=excluded.linked_at",
    )
    .bind(link.account_id.as_str())
    .bind(link.group_id.as_str())
    .bind(link.thread_id.as_str())
    .bind(link.linked_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn list_threads_by_group(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    group_id: &GroupId,
) -> Result<Vec<GroupThread>, PortError> {
    let rows = sqlx::query(
        "SELECT account_id, group_id, thread_id, linked_at FROM group_thread
         WHERE account_id = ? AND group_id = ? ORDER BY thread_id",
    )
    .bind(account_id.as_str())
    .bind(group_id.as_str())
    .fetch_all(&mut *conn)
    .await?;
    use sqlx::Row;
    rows.iter()
        .map(|row| {
            Ok(GroupThread {
                account_id: AccountId::new(row.try_get::<String, _>("account_id")?),
                group_id: GroupId::new(row.try_get::<String, _>("group_id")?),
                thread_id: ThreadId::new(row.try_get::<String, _>("thread_id")?),
                linked_at: row.try_get("linked_at")?,
            })
        })
        .collect()
}
