//! SQLite messages 模块函数；连接与事务由调用方持有。

use crate::application::error::PortError;
use crate::domain::entities::MessageWithRecipients;
use crate::domain::value_objects::{
    AccountId, EmailAddress, GraphImmutableId, MessageId, SessionId,
};
use sqlx::{QueryBuilder, Sqlite, SqliteConnection};

pub async fn get(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    id: &MessageId,
) -> Result<Option<MessageWithRecipients>, PortError> {
    let row = sqlx::query("SELECT * FROM messages WHERE account_id = ? AND id = ?")
        .bind(account_id.as_str())
        .bind(id.as_str())
        .fetch_optional(&mut *conn)
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let message = super::rows::message(&row)?;
    let recipients = load_recipients(conn, account_id, &message.id).await?;
    Ok(Some(MessageWithRecipients {
        message,
        recipients,
    }))
}

pub async fn get_by_graph_id(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    graph_id: &GraphImmutableId,
) -> Result<Option<MessageWithRecipients>, PortError> {
    let row = sqlx::query("SELECT * FROM messages WHERE account_id = ? AND graph_id = ?")
        .bind(account_id.as_str())
        .bind(graph_id.as_str())
        .fetch_optional(&mut *conn)
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let message = super::rows::message(&row)?;
    let id = message.id.clone();
    let recipients = load_recipients(conn, account_id, &id).await?;
    Ok(Some(MessageWithRecipients {
        message,
        recipients,
    }))
}

pub async fn find_by_internet_message_id(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    internet_message_id: &str,
) -> Result<Option<MessageWithRecipients>, PortError> {
    let trimmed = internet_message_id.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let row = sqlx::query(
        "SELECT * FROM messages WHERE account_id = ? AND internet_message_id = ? LIMIT 1",
    )
    .bind(account_id.as_str())
    .bind(trimmed)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let message = super::rows::message(&row)?;
    let id = message.id.clone();
    let recipients = load_recipients(conn, account_id, &id).await?;
    Ok(Some(MessageWithRecipients {
        message,
        recipients,
    }))
}

async fn load_recipients(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Vec<crate::domain::entities::MessageRecipient>, PortError> {
    sqlx::query(
        "SELECT * FROM message_recipients WHERE account_id = ? AND message_id = ? ORDER BY role, seq",
    )
    .bind(account_id.as_str())
    .bind(message_id.as_str())
    .fetch_all(&mut *conn)
    .await?
    .iter()
    .map(super::rows::messagerecipient)
    .collect::<Result<Vec<_>, _>>()
    .map_err(Into::into)
}

pub async fn upsert_with_recipients(
    conn: &mut SqliteConnection,
    bundle: &MessageWithRecipients,
) -> Result<(), PortError> {
    let m = &bundle.message;
    sqlx::query(
        "INSERT INTO messages (
            id, account_id, session_id, graph_id, graph_change_key, internet_message_id,
            in_reply_to, subject, body_preview, body_content_type, body_content,
            from_address, from_name, received_at, sent_at, created_at_graph,
            is_read, is_draft, is_outbound, has_attachments, importance,
            folder_id, parent_folder_name, sync_status, local_updated_at, remote_updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(account_id, id) DO UPDATE SET
            session_id=excluded.session_id,
            graph_id=excluded.graph_id,
            graph_change_key=excluded.graph_change_key,
            internet_message_id=excluded.internet_message_id,
            in_reply_to=excluded.in_reply_to,
            subject=excluded.subject,
            body_preview=excluded.body_preview,
            body_content_type=excluded.body_content_type,
            body_content=excluded.body_content,
            from_address=excluded.from_address,
            from_name=excluded.from_name,
            received_at=excluded.received_at,
            sent_at=excluded.sent_at,
            created_at_graph=excluded.created_at_graph,
            is_read=excluded.is_read,
            is_draft=excluded.is_draft,
            is_outbound=excluded.is_outbound,
            has_attachments=excluded.has_attachments,
            importance=excluded.importance,
            folder_id=excluded.folder_id,
            parent_folder_name=excluded.parent_folder_name,
            sync_status=excluded.sync_status,
            local_updated_at=excluded.local_updated_at,
            remote_updated_at=excluded.remote_updated_at",
    )
    .bind(m.id.as_str())
    .bind(m.account_id.as_str())
    .bind(m.session_id.as_str())
    .bind(m.graph_id.as_ref().map(|g| g.as_str()))
    .bind(&m.graph_change_key)
    .bind(&m.internet_message_id)
    .bind(&m.in_reply_to)
    .bind(&m.subject)
    .bind(&m.body_preview)
    .bind(m.body_content_type.as_str())
    .bind(&m.body_content)
    .bind(m.from_address.as_ref().map(|a| a.as_str()))
    .bind(&m.from_name)
    .bind(m.received_at)
    .bind(m.sent_at)
    .bind(m.created_at_graph)
    .bind(m.is_read)
    .bind(m.is_draft)
    .bind(m.is_outbound)
    .bind(m.has_attachments)
    .bind(m.importance.as_str())
    .bind(m.folder_id.as_ref().map(|f| f.as_str()))
    .bind(&m.parent_folder_name)
    .bind(m.sync_status.as_str())
    .bind(m.local_updated_at)
    .bind(m.remote_updated_at)
    .execute(&mut *conn)
    .await?;

    sqlx::query("DELETE FROM message_recipients WHERE account_id = ? AND message_id = ?")
        .bind(m.account_id.as_str())
        .bind(m.id.as_str())
        .execute(&mut *conn)
        .await?;

    for r in &bundle.recipients {
        sqlx::query(
            "INSERT INTO message_recipients (
                account_id, message_id, role, seq, normalized_address, display_name
            ) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(r.account_id.as_str())
        .bind(r.message_id.as_str())
        .bind(r.role.as_str())
        .bind(r.seq)
        .bind(r.normalized_address.as_str())
        .bind(&r.display_name)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

pub async fn set_session_id(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_id: &MessageId,
    session_id: &SessionId,
) -> Result<(), PortError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    sqlx::query(
        "UPDATE messages SET session_id = ?, local_updated_at = ?
         WHERE account_id = ? AND id = ?",
    )
    .bind(session_id.as_str())
    .bind(now)
    .bind(account_id.as_str())
    .bind(message_id.as_str())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn mark_read(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_ids: &[MessageId],
    is_read: bool,
) -> Result<Vec<MessageId>, PortError> {
    let mut changed = Vec::new();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    for id in message_ids {
        let result = sqlx::query(
            "UPDATE messages SET is_read = ?, local_updated_at = ?
             WHERE account_id = ? AND id = ? AND is_read != ?",
        )
        .bind(is_read)
        .bind(now)
        .bind(account_id.as_str())
        .bind(id.as_str())
        .bind(is_read)
        .execute(&mut *conn)
        .await?;
        if result.rows_affected() > 0 {
            changed.push(id.clone());
        }
    }
    Ok(changed)
}

pub async fn mark_deleted_remote(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_ids: &[MessageId],
) -> Result<(), PortError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    for id in message_ids {
        sqlx::query(
            "UPDATE messages SET sync_status = 'deleted_remote', local_updated_at = ?
             WHERE account_id = ? AND id = ?",
        )
        .bind(now)
        .bind(account_id.as_str())
        .bind(id.as_str())
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

pub async fn delete_physical(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_ids: &[MessageId],
) -> Result<(), PortError> {
    for id in message_ids {
        sqlx::query("DELETE FROM messages WHERE account_id = ? AND id = ?")
            .bind(account_id.as_str())
            .bind(id.as_str())
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

/// 列出消息所属 session_id（去重）
pub async fn list_session_ids(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_ids: &[MessageId],
) -> Result<Vec<SessionId>, PortError> {
    let mut out = Vec::new();
    for id in message_ids {
        let sid: Option<String> = sqlx::query_scalar(
            "SELECT session_id FROM messages WHERE account_id = ? AND id = ?",
        )
        .bind(account_id.as_str())
        .bind(id.as_str())
        .fetch_optional(&mut *conn)
        .await?;
        if let Some(s) = sid {
            out.push(SessionId::new(s));
        }
    }
    out.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    out.dedup();
    Ok(out)
}

/// 列出会话内非远端删除的邮件 id。
pub async fn list_ids_by_session(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    session_id: &SessionId,
) -> Result<Vec<MessageId>, PortError> {
    let rows = sqlx::query_scalar::<_, String>(
        "SELECT id FROM messages
         WHERE account_id = ? AND session_id = ? AND sync_status != 'deleted_remote'
         ORDER BY local_updated_at DESC",
    )
    .bind(account_id.as_str())
    .bind(session_id.as_str())
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows.into_iter().map(MessageId::new).collect())
}

/// 发件人或收件人命中给定地址的邮件（别名曾被当成对端时的纠正范围）。
pub async fn list_ids_involving_addresses(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    addresses: &[EmailAddress],
) -> Result<Vec<MessageId>, PortError> {
    if addresses.is_empty() {
        return Ok(Vec::new());
    }
    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
        "SELECT DISTINCT m.id FROM messages m
         WHERE m.account_id = ",
    );
    qb.push_bind(account_id.as_str());
    qb.push(" AND m.sync_status != 'deleted_remote' AND (m.from_address IN (");
    {
        let mut separated = qb.separated(", ");
        for addr in addresses {
            separated.push_bind(addr.as_str());
        }
    }
    qb.push(
        ") OR EXISTS (
            SELECT 1 FROM message_recipients r
            WHERE r.account_id = m.account_id AND r.message_id = m.id
              AND r.normalized_address IN (",
    );
    {
        let mut separated = qb.separated(", ");
        for addr in addresses {
            separated.push_bind(addr.as_str());
        }
    }
    qb.push(")) ORDER BY m.local_updated_at DESC");
    let rows = qb.build_query_scalar::<String>().fetch_all(&mut *conn).await?;
    Ok(rows.into_iter().map(MessageId::new).collect())
}
