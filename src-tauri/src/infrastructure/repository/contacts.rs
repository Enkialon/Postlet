//! SQLite contacts 模块函数；连接与事务由调用方持有。

use crate::application::dto::{ContactListQuery, CursorPage};
use crate::application::error::PortError;
use crate::domain::entities::Contact;
use crate::domain::value_objects::{AccountId, ContactId, EmailAddress};
use sqlx::SqliteConnection;

pub async fn get(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    id: &ContactId,
) -> Result<Option<Contact>, PortError> {
    let row = sqlx::query("SELECT * FROM contacts WHERE account_id = ? AND id = ?")
        .bind(account_id.as_str())
        .bind(id.as_str())
        .fetch_optional(&mut *conn)
        .await?;
    row.as_ref()
        .map(super::rows::contact)
        .transpose()
        .map_err(Into::into)
}

pub async fn get_by_address(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    address: &EmailAddress,
) -> Result<Option<Contact>, PortError> {
    let row = sqlx::query("SELECT * FROM contacts WHERE account_id = ? AND normalized_address = ?")
        .bind(account_id.as_str())
        .bind(address.as_str())
        .fetch_optional(&mut *conn)
        .await?;
    row.as_ref()
        .map(super::rows::contact)
        .transpose()
        .map_err(Into::into)
}

pub async fn list(
    conn: &mut SqliteConnection,
    query: &ContactListQuery,
) -> Result<CursorPage<Contact>, PortError> {
    if query.limit == 0 || query.limit > 500 {
        return Err(PortError::Conflict {
            message: "分页大小须为 1..500".into(),
        });
    }
    let account_filter = query.account_id.as_ref().map(|a| a.as_str());
    let (cursor_account, cursor_id) = parse_entity_cursor(query.cursor.as_deref())?;
    let rows = sqlx::query(
        "SELECT * FROM contacts
         WHERE (? IS NULL OR account_id = ?)
           AND (
             ? IS NULL
             OR account_id > ?
             OR (account_id = ? AND id > ?)
           )
           AND (
             ? IS NULL
             OR instr(lower(display_name), lower(?)) = 1
             OR instr(normalized_address, lower(?)) = 1
           )
         ORDER BY account_id ASC, id ASC
         LIMIT ?",
    )
    .bind(account_filter)
    .bind(account_filter)
    .bind(cursor_id.as_deref())
    .bind(cursor_account.as_deref().unwrap_or(""))
    .bind(cursor_account.as_deref().unwrap_or(""))
    .bind(cursor_id.as_deref())
    .bind(query.search.as_deref())
    .bind(query.search.as_deref())
    .bind(query.search.as_deref())
    .bind(i64::from(query.limit) + 1)
    .fetch_all(&mut *conn)
    .await?;
    let has_more = rows.len() > query.limit as usize;
    let items = rows
        .iter()
        .take(query.limit as usize)
        .map(super::rows::contact)
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

pub async fn upsert(conn: &mut SqliteConnection, contact: &Contact) -> Result<(), PortError> {
    sqlx::query("INSERT INTO contacts (id, account_id, graph_id, display_name, normalized_address, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT(account_id, id) DO UPDATE SET graph_id=excluded.graph_id, display_name=excluded.display_name, normalized_address=excluded.normalized_address, updated_at=excluded.updated_at")
        .bind(contact.id.as_str()).bind(contact.account_id.as_str()).bind(&contact.graph_id)
        .bind(&contact.display_name).bind(contact.normalized_address.as_str())
        .bind(contact.created_at).bind(contact.updated_at).execute(conn).await?;
    Ok(())
}

pub async fn delete(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    id: &ContactId,
) -> Result<(), PortError> {
    let _ = (conn, account_id, id);
    Err(PortError::Storage {
        message: "尚未实现: repository::contacts::delete".into(),
    })
}

/// 游标 `account_id:id`；兼容旧纯 `id`
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
