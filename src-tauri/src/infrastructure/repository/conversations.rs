//! SQLite conversations 模块函数；连接与事务由调用方持有。

use crate::application::dto::{
    ConversationListItem, ConversationListQuery, ConversationMessagesQuery, CursorPage,
    MessagePreview,
};
use crate::application::error::PortError;
use crate::domain::entities::{Conversation, ConversationMessageLink, MessageWithRecipients};
use crate::domain::value_objects::{
    AccountId, ContactId, ConversationId, EmailAddress, GroupId, MessageId, PageDirection,
};
use crate::infrastructure::repository::accounts;
use sqlx::{QueryBuilder, SqliteConnection};

pub async fn get(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    id: &ConversationId,
) -> Result<Option<Conversation>, PortError> {
    let row = sqlx::query("SELECT * FROM conversations WHERE account_id = ? AND id = ?")
        .bind(account_id.as_str())
        .bind(id.as_str())
        .fetch_optional(&mut *conn)
        .await?;
    row.as_ref()
        .map(super::rows::conversation)
        .transpose()
        .map_err(Into::into)
}

pub async fn get_list_item(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    id: &ConversationId,
) -> Result<Option<ConversationListItem>, PortError> {
    let Some(conversation) = get(conn, account_id, id).await? else {
        return Ok(None);
    };
    Ok(Some(enrich_item(conn, conversation).await?))
}

pub async fn get_contact_conversation(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    address: &EmailAddress,
) -> Result<Option<Conversation>, PortError> {
    let row = sqlx::query(
        "SELECT * FROM conversations WHERE account_id = ? AND kind = 'contact' AND normalized_address = ?",
    )
    .bind(account_id.as_str())
    .bind(address.as_str())
    .fetch_optional(&mut *conn)
    .await?;
    row.as_ref()
        .map(super::rows::conversation)
        .transpose()
        .map_err(Into::into)
}

pub async fn get_group_conversation(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    group_id: &GroupId,
) -> Result<Option<Conversation>, PortError> {
    let row = sqlx::query(
        "SELECT * FROM conversations WHERE account_id = ? AND kind = 'group' AND group_id = ?",
    )
    .bind(account_id.as_str())
    .bind(group_id.as_str())
    .fetch_optional(&mut *conn)
    .await?;
    row.as_ref()
        .map(super::rows::conversation)
        .transpose()
        .map_err(Into::into)
}

pub async fn list(
    conn: &mut SqliteConnection,
    query: &ConversationListQuery,
) -> Result<CursorPage<ConversationListItem>, PortError> {
    let kind = query.kind.map(|k| k.as_str());
    if query.limit == 0 || query.limit > 500 {
        return Err(PortError::Conflict {
            message: "分页大小须为 1..500".into(),
        });
    }
    // 按最近消息时间倒序；空会话排后；同时间用 account_id+id 稳定排序。
    // 游标："{last_message_at}:{account_id}:{id}"（兼容旧 "{at}:{id}"）
    let (cursor_at, mut cursor_account, cursor_id) =
        parse_conv_cursor(query.cursor.as_deref())?;
    let account_ids = accounts::resolve_list_account_ids(conn, query.account_id.as_ref()).await?;
    if account_ids.is_empty() {
        return Ok(CursorPage::empty());
    }
    // 旧游标无 account 时，单账号查询用过滤账号补齐
    if cursor_account.is_none() && account_ids.len() == 1 {
        cursor_account = Some(account_ids[0].as_str().to_owned());
    }

    let mut qb = QueryBuilder::new("SELECT * FROM conversations WHERE ");
    accounts::push_account_id_in(&mut qb, &account_ids);
    qb.push(" AND hidden_at IS NULL AND (");
    if let Some(kind) = kind {
        qb.push("kind = ");
        qb.push_bind(kind);
    } else {
        qb.push("1 = 1");
    }
    qb.push(") AND (");
    if cursor_id.is_none() {
        qb.push("1 = 1");
    } else {
        qb.push("(COALESCE(last_message_at, -1) < ");
        qb.push_bind(cursor_at.unwrap_or(i64::MAX));
        qb.push(") OR (COALESCE(last_message_at, -1) = ");
        qb.push_bind(cursor_at.unwrap_or(-1));
        qb.push(" AND account_id > ");
        qb.push_bind(cursor_account.as_deref().unwrap_or(""));
        qb.push(") OR (COALESCE(last_message_at, -1) = ");
        qb.push_bind(cursor_at.unwrap_or(-1));
        qb.push(" AND account_id = ");
        qb.push_bind(cursor_account.as_deref().unwrap_or(""));
        qb.push(" AND id > ");
        qb.push_bind(cursor_id.as_deref());
        qb.push(")");
    }
    qb.push(
        ") ORDER BY (last_message_at IS NULL), last_message_at DESC, account_id ASC, id ASC LIMIT ",
    );
    qb.push_bind(i64::from(query.limit) + 1);

    let rows = qb.build().fetch_all(&mut *conn).await?;

    let has_more = rows.len() > query.limit as usize;
    let mut conversations = rows
        .iter()
        .take(query.limit as usize)
        .map(super::rows::conversation)
        .collect::<Result<Vec<_>, _>>()?;
    let next_cursor = if has_more {
        conversations.last().map(|c| {
            format!(
                "{}:{}:{}",
                c.last_message_at.unwrap_or(-1),
                c.account_id.as_str(),
                c.id.as_str()
            )
        })
    } else {
        None
    };
    let mut items = Vec::with_capacity(conversations.len());
    for conversation in conversations.drain(..) {
        items.push(enrich_item(conn, conversation).await?);
    }
    Ok(CursorPage {
        items,
        next_cursor,
        has_more,
    })
}

fn parse_conv_cursor(
    cursor: Option<&str>,
) -> Result<(Option<i64>, Option<String>, Option<String>), PortError> {
    let Some(raw) = cursor else {
        return Ok((None, None, None));
    };
    let parts: Vec<&str> = raw.splitn(3, ':').collect();
    match parts.as_slice() {
        [at, account_id, id] => {
            let at: i64 = at.parse().map_err(|_| PortError::Conflict {
                message: "无效会话游标时间".into(),
            })?;
            Ok((Some(at), Some((*account_id).to_owned()), Some((*id).to_owned())))
        }
        // 兼容旧游标 "{at}:{id}"
        [at, id] => {
            let at: i64 = at.parse().map_err(|_| PortError::Conflict {
                message: "无效会话游标时间".into(),
            })?;
            Ok((Some(at), None, Some((*id).to_owned())))
        }
        _ => Err(PortError::Conflict {
            message: "无效会话游标".into(),
        }),
    }
}

async fn enrich_item(
    conn: &mut SqliteConnection,
    conversation: Conversation,
) -> Result<ConversationListItem, PortError> {
    let contact_display_name = if let Some(ref cid) = conversation.contact_id {
        sqlx::query_scalar::<_, String>(
            "SELECT display_name FROM contacts WHERE account_id = ? AND id = ?",
        )
        .bind(conversation.account_id.as_str())
        .bind(cid.as_str())
        .fetch_optional(&mut *conn)
        .await?
        .filter(|s| !s.is_empty())
    } else {
        None
    };
    let group_name = if let Some(ref gid) = conversation.group_id {
        sqlx::query_scalar::<_, String>(
            "SELECT name FROM groups WHERE account_id = ? AND id = ?",
        )
        .bind(conversation.account_id.as_str())
        .bind(gid.as_str())
        .fetch_optional(&mut *conn)
        .await?
    } else {
        None
    };
    let last_message_preview = if let Some(ref mid) = conversation.last_message_id {
        let row = sqlx::query(
            "SELECT * FROM messages WHERE account_id = ? AND id = ? AND sync_status != 'deleted_remote'",
        )
        .bind(conversation.account_id.as_str())
        .bind(mid.as_str())
        .fetch_optional(&mut *conn)
        .await?;
        match row {
            Some(row) => {
                let m = super::rows::message(&row)?;
                Some(MessagePreview {
                    id: m.id,
                    subject: m.subject,
                    body_preview: m.body_preview,
                    from_address: m.from_address,
                    from_name: m.from_name,
                    received_at: m.received_at,
                    sent_at: m.sent_at,
                    is_read: m.is_read,
                    is_outbound: m.is_outbound,
                    has_attachments: m.has_attachments,
                    importance: m.importance,
                })
            }
            None => None,
        }
    } else {
        None
    };
    Ok(ConversationListItem {
        conversation,
        last_message_preview,
        contact_display_name,
        group_name,
    })
}

pub async fn upsert(
    conn: &mut SqliteConnection,
    conversation: &Conversation,
) -> Result<(), PortError> {
    sqlx::query(
        "INSERT INTO conversations (
            id, account_id, kind, normalized_address, contact_id, group_id,
            last_message_at, last_message_id, unread_count, hidden_at, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(account_id, id) DO UPDATE SET
            kind=excluded.kind,
            normalized_address=excluded.normalized_address,
            contact_id=excluded.contact_id,
            group_id=excluded.group_id,
            last_message_at=excluded.last_message_at,
            last_message_id=excluded.last_message_id,
            unread_count=excluded.unread_count,
            updated_at=excluded.updated_at",
        // 冲突更新不改 hidden_at，避免投影重写冲掉用户隐藏
    )
    .bind(conversation.id.as_str())
    .bind(conversation.account_id.as_str())
    .bind(conversation.kind.as_str())
    .bind(
        conversation
            .normalized_address
            .as_ref()
            .map(|a| a.as_str()),
    )
    .bind(conversation.contact_id.as_ref().map(|c| c.as_str()))
    .bind(conversation.group_id.as_ref().map(|g| g.as_str()))
    .bind(conversation.last_message_at)
    .bind(conversation.last_message_id.as_ref().map(|m| m.as_str()))
    .bind(conversation.unread_count)
    .bind(conversation.hidden_at)
    .bind(conversation.created_at)
    .bind(conversation.updated_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// 设置或清除本地隐藏。
pub async fn set_hidden_at(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    conversation_id: &ConversationId,
    hidden_at: Option<i64>,
    now: i64,
) -> Result<(), PortError> {
    let result = sqlx::query(
        "UPDATE conversations
         SET hidden_at = ?, updated_at = ?
         WHERE account_id = ? AND id = ?",
    )
    .bind(hidden_at)
    .bind(now)
    .bind(account_id.as_str())
    .bind(conversation_id.as_str())
    .execute(&mut *conn)
    .await?;
    if result.rows_affected() == 0 {
        return Err(PortError::Conflict {
            message: format!("会话不存在: {conversation_id}"),
        });
    }
    Ok(())
}

pub async fn clear_contact_id_refs(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    contact_id: &ContactId,
) -> Result<(), PortError> {
    sqlx::query(
        "UPDATE conversations SET contact_id = NULL, updated_at = ?
         WHERE account_id = ? AND contact_id = ?",
    )
    .bind(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0),
    )
    .bind(account_id.as_str())
    .bind(contact_id.as_str())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn attach_messages(
    conn: &mut SqliteConnection,
    links: &[ConversationMessageLink],
) -> Result<(), PortError> {
    for link in links {
        sqlx::query(
            "INSERT INTO conversation_messages (account_id, conversation_id, message_id, attached_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(account_id, conversation_id, message_id) DO UPDATE SET attached_at=excluded.attached_at",
        )
        .bind(link.account_id.as_str())
        .bind(link.conversation_id.as_str())
        .bind(link.message_id.as_str())
        .bind(link.attached_at)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

pub async fn detach_messages(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    conversation_id: &ConversationId,
    message_ids: &[MessageId],
) -> Result<(), PortError> {
    for id in message_ids {
        sqlx::query(
            "DELETE FROM conversation_messages WHERE account_id = ? AND conversation_id = ? AND message_id = ?",
        )
        .bind(account_id.as_str())
        .bind(conversation_id.as_str())
        .bind(id.as_str())
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

pub async fn list_message_ids(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    conversation_id: &ConversationId,
) -> Result<Vec<MessageId>, PortError> {
    let rows = sqlx::query_scalar::<_, String>(
        "SELECT message_id FROM conversation_messages WHERE account_id = ? AND conversation_id = ?",
    )
    .bind(account_id.as_str())
    .bind(conversation_id.as_str())
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows.into_iter().map(MessageId::new).collect())
}

pub async fn list_messages(
    conn: &mut SqliteConnection,
    query: &ConversationMessagesQuery,
) -> Result<CursorPage<MessageWithRecipients>, PortError> {
    if query.limit == 0 || query.limit > 500 {
        return Err(PortError::Conflict {
            message: "分页大小须为 1..500".into(),
        });
    }
    let (cursor_at, cursor_id) = parse_msg_cursor(query.cursor.as_deref())?;
    let direction = query.direction;
    // older：比游标更旧（时间更小）；newer：更晚。无游标时从最新开始取 older。
    let rows = match direction {
        PageDirection::Older => {
            sqlx::query(
                "SELECT m.* FROM conversation_messages cm
                 INNER JOIN messages m
                   ON m.account_id = cm.account_id AND m.id = cm.message_id
                 WHERE cm.account_id = ? AND cm.conversation_id = ?
                   AND m.sync_status != 'deleted_remote'
                   AND (
                     ? IS NULL
                     OR COALESCE(m.received_at, m.sent_at, 0) < ?
                     OR (COALESCE(m.received_at, m.sent_at, 0) = ? AND m.id < ?)
                   )
                 ORDER BY COALESCE(m.received_at, m.sent_at, 0) DESC, m.id DESC
                 LIMIT ?",
            )
            .bind(query.account_id.as_str())
            .bind(query.conversation_id.as_str())
            .bind(cursor_id.as_deref())
            .bind(cursor_at.unwrap_or(i64::MAX))
            .bind(cursor_at.unwrap_or(i64::MAX))
            .bind(cursor_id.as_deref())
            .bind(i64::from(query.limit) + 1)
            .fetch_all(&mut *conn)
            .await?
        }
        PageDirection::Newer => {
            sqlx::query(
                "SELECT m.* FROM conversation_messages cm
                 INNER JOIN messages m
                   ON m.account_id = cm.account_id AND m.id = cm.message_id
                 WHERE cm.account_id = ? AND cm.conversation_id = ?
                   AND m.sync_status != 'deleted_remote'
                   AND (
                     ? IS NULL
                     OR COALESCE(m.received_at, m.sent_at, 0) > ?
                     OR (COALESCE(m.received_at, m.sent_at, 0) = ? AND m.id > ?)
                   )
                 ORDER BY COALESCE(m.received_at, m.sent_at, 0) ASC, m.id ASC
                 LIMIT ?",
            )
            .bind(query.account_id.as_str())
            .bind(query.conversation_id.as_str())
            .bind(cursor_id.as_deref())
            .bind(cursor_at.unwrap_or(-1))
            .bind(cursor_at.unwrap_or(-1))
            .bind(cursor_id.as_deref())
            .bind(i64::from(query.limit) + 1)
            .fetch_all(&mut *conn)
            .await?
        }
    };

    let has_more = rows.len() > query.limit as usize;
    let message_rows: Vec<_> = rows.into_iter().take(query.limit as usize).collect();
    let mut items = Vec::with_capacity(message_rows.len());
    for row in &message_rows {
        let message = super::rows::message(row)?;
        let recipients = sqlx::query(
            "SELECT * FROM message_recipients WHERE account_id = ? AND message_id = ? ORDER BY role, seq",
        )
        .bind(query.account_id.as_str())
        .bind(message.id.as_str())
        .fetch_all(&mut *conn)
        .await?
        .iter()
        .map(super::rows::messagerecipient)
        .collect::<Result<Vec<_>, _>>()?;
        items.push(MessageWithRecipients {
            message,
            recipients,
        });
    }
    let next_cursor = if has_more {
        items.last().map(|item| {
            let at = item
                .message
                .received_at
                .or(item.message.sent_at)
                .unwrap_or(0);
            format!("{}:{}", at, item.message.id.as_str())
        })
    } else {
        None
    };
    Ok(CursorPage {
        items,
        next_cursor,
        has_more,
    })
}

fn parse_msg_cursor(cursor: Option<&str>) -> Result<(Option<i64>, Option<String>), PortError> {
    let Some(raw) = cursor else {
        return Ok((None, None));
    };
    let (at, id) = raw.split_once(':').ok_or_else(|| PortError::Conflict {
        message: "无效消息游标".into(),
    })?;
    let at: i64 = at.parse().map_err(|_| PortError::Conflict {
        message: "无效消息游标时间".into(),
    })?;
    Ok((Some(at), Some(id.to_owned())))
}

pub async fn recompute_projection(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    conversation_id: &ConversationId,
) -> Result<Conversation, PortError> {
    let last = sqlx::query(
        "SELECT m.id, COALESCE(m.received_at, m.sent_at) AS at
         FROM conversation_messages cm
         INNER JOIN messages m ON m.account_id = cm.account_id AND m.id = cm.message_id
         WHERE cm.account_id = ? AND cm.conversation_id = ?
           AND m.sync_status != 'deleted_remote'
         ORDER BY COALESCE(m.received_at, m.sent_at, 0) DESC, m.id DESC
         LIMIT 1",
    )
    .bind(account_id.as_str())
    .bind(conversation_id.as_str())
    .fetch_optional(&mut *conn)
    .await?;

    let (last_message_id, last_message_at): (Option<String>, Option<i64>) = match last {
        Some(row) => {
            use sqlx::Row;
            (
                Some(row.try_get::<String, _>("id")?),
                row.try_get::<Option<i64>, _>("at")?,
            )
        }
        None => (None, None),
    };

    let unread: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM conversation_messages cm
         INNER JOIN messages m ON m.account_id = cm.account_id AND m.id = cm.message_id
         WHERE cm.account_id = ? AND cm.conversation_id = ?
           AND m.sync_status != 'deleted_remote' AND m.is_read = 0",
    )
    .bind(account_id.as_str())
    .bind(conversation_id.as_str())
    .fetch_one(&mut *conn)
    .await?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    // 有比隐藏时刻更新的消息 → 自动取消隐藏（再次出现在列表）
    sqlx::query(
        "UPDATE conversations
         SET last_message_id = ?,
             last_message_at = ?,
             unread_count = ?,
             updated_at = ?,
             hidden_at = CASE
               WHEN hidden_at IS NOT NULL
                    AND ? IS NOT NULL
                    AND ? > hidden_at
               THEN NULL
               ELSE hidden_at
             END
         WHERE account_id = ? AND id = ?",
    )
    .bind(last_message_id.as_deref())
    .bind(last_message_at)
    .bind(unread)
    .bind(now)
    .bind(last_message_at)
    .bind(last_message_at)
    .bind(account_id.as_str())
    .bind(conversation_id.as_str())
    .execute(&mut *conn)
    .await?;

    get(conn, account_id, conversation_id)
        .await?
        .ok_or_else(|| PortError::Conflict {
            message: format!("会话不存在: {conversation_id}"),
        })
}

pub async fn find_conversation_ids_by_message(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Vec<ConversationId>, PortError> {
    let rows = sqlx::query_scalar::<_, String>(
        "SELECT conversation_id FROM conversation_messages WHERE account_id = ? AND message_id = ?",
    )
    .bind(account_id.as_str())
    .bind(message_id.as_str())
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows.into_iter().map(ConversationId::new).collect())
}
