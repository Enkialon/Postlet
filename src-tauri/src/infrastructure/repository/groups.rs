//! SQLite groups 模块函数；连接与事务由调用方持有。

use crate::application::dto::{CursorPage, GroupListQuery};
use crate::application::error::PortError;
use crate::domain::entities::{Group, GroupParticipant};
use crate::domain::value_objects::{AccountId, GroupId};
use crate::infrastructure::repository::accounts;
use sqlx::{QueryBuilder, SqliteConnection};

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

pub async fn list(
    conn: &mut SqliteConnection,
    query: &GroupListQuery,
) -> Result<CursorPage<Group>, PortError> {
    if query.limit == 0 || query.limit > 500 {
        return Err(PortError::Conflict {
            message: "分页大小须为 1..500".into(),
        });
    }
    let account_ids = accounts::resolve_list_account_ids(conn, query.account_id.as_ref()).await?;
    if account_ids.is_empty() {
        return Ok(CursorPage::empty());
    }
    let (cursor_account, cursor_id) = parse_entity_cursor(query.cursor.as_deref())?;

    let mut qb = QueryBuilder::new("SELECT * FROM groups WHERE ");
    accounts::push_account_id_in(&mut qb, &account_ids);
    qb.push(" AND (");
    if cursor_id.is_none() {
        qb.push("1 = 1");
    } else {
        qb.push("account_id > ");
        qb.push_bind(cursor_account.as_deref().unwrap_or(""));
        qb.push(" OR (account_id = ");
        qb.push_bind(cursor_account.as_deref().unwrap_or(""));
        qb.push(" AND id > ");
        qb.push_bind(cursor_id.as_deref());
        qb.push(")");
    }
    qb.push(") ORDER BY account_id ASC, id ASC LIMIT ");
    qb.push_bind(i64::from(query.limit) + 1);

    let rows = qb.build().fetch_all(&mut *conn).await?;
    let has_more = rows.len() > query.limit as usize;
    let items = rows
        .iter()
        .take(query.limit as usize)
        .map(super::rows::group)
        .collect::<Result<Vec<_>, _>>()?;
    let next_cursor = if has_more {
        items
            .last()
            .map(|x| format!("{}:{}", x.account_id.as_str(), x.id.as_str()))
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
            created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(group.id.as_str())
    .bind(group.account_id.as_str())
    .bind(&group.name)
    .bind(group.name_is_manual)
    .bind(group.source.as_str())
    .bind(group.send_members_is_manual)
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
            created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(account_id, id) DO UPDATE SET
            name=excluded.name,
            name_is_manual=excluded.name_is_manual,
            source=excluded.source,
            send_members_is_manual=excluded.send_members_is_manual,
            updated_at=excluded.updated_at",
    )
    .bind(group.id.as_str())
    .bind(group.account_id.as_str())
    .bind(&group.name)
    .bind(group.name_is_manual)
    .bind(group.source.as_str())
    .bind(group.send_members_is_manual)
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
            updated_at = ?
         WHERE account_id = ? AND id = ?",
    )
    .bind(&group.name)
    .bind(group.name_is_manual)
    .bind(group.source.as_str())
    .bind(group.send_members_is_manual)
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

fn parse_entity_cursor(
    cursor: Option<&str>,
) -> Result<(Option<String>, Option<String>), PortError> {
    let Some(raw) = cursor else {
        return Ok((None, None));
    };
    if let Some((account_id, id)) = raw.split_once(':') {
        Ok((Some(account_id.to_owned()), Some(id.to_owned())))
    } else {
        Ok((None, Some(raw.to_owned())))
    }
}
