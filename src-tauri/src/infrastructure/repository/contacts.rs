//! SQLite contacts 模块函数；连接与事务由调用方持有。

use crate::application::dto::{ContactListQuery, CursorPage};
use crate::application::error::PortError;
use crate::domain::entities::Contact;
use crate::domain::value_objects::{AccountId, ContactId, EmailAddress};
use crate::infrastructure::repository::accounts;
use sqlx::{QueryBuilder, SqliteConnection};

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
    let account_ids = accounts::resolve_list_account_ids(conn, query.account_id.as_ref()).await?;
    if account_ids.is_empty() {
        return Ok(CursorPage::empty());
    }
    let (cursor_account, cursor_id) = parse_entity_cursor(query.cursor.as_deref())?;

    let mut qb = QueryBuilder::new("SELECT * FROM contacts WHERE ");
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
    qb.push(") AND (");
    if let Some(search) = query.search.as_deref() {
        qb.push("instr(lower(display_name), lower(");
        qb.push_bind(search);
        qb.push(")) = 1 OR instr(normalized_address, lower(");
        qb.push_bind(search);
        qb.push(")) = 1");
    } else {
        qb.push("1 = 1");
    }
    qb.push(") ORDER BY account_id ASC, id ASC LIMIT ");
    qb.push_bind(i64::from(query.limit) + 1);

    let rows = qb.build().fetch_all(&mut *conn).await?;
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

pub async fn get_by_graph_id(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    graph_id: &str,
) -> Result<Option<Contact>, PortError> {
    let row = sqlx::query(
        "SELECT * FROM contacts WHERE account_id = ? AND graph_id = ?",
    )
    .bind(account_id.as_str())
    .bind(graph_id)
    .fetch_optional(&mut *conn)
    .await?;
    row.as_ref()
        .map(super::rows::contact)
        .transpose()
        .map_err(Into::into)
}

pub async fn delete(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    id: &ContactId,
) -> Result<(), PortError> {
    // 清空会话上的 contact_id 引用，避免悬挂外键语义
    super::sessions::clear_contact_id_refs(conn, account_id, id).await?;
    sqlx::query("DELETE FROM contacts WHERE account_id = ? AND id = ?")
        .bind(account_id.as_str())
        .bind(id.as_str())
        .execute(&mut *conn)
        .await?;
    Ok(())
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
