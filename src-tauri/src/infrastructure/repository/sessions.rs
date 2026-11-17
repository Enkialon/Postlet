//! SQLite sessions 模块函数；连接与事务由调用方持有。

use crate::domain::entities::{MessageWithRecipients, Session};
use crate::domain::value_objects::{
    AccountId, ContactId, EmailAddress, GroupId, MessageId, PageDirection, SessionId, SessionKind,
};
use crate::infrastructure::error::InfraError;
use crate::infrastructure::repository::accounts;
use crate::query_page::CursorPage;
use sqlx::{QueryBuilder, SqliteConnection};

pub async fn get(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    id: &SessionId,
) -> Result<Option<Session>, InfraError> {
    let row = sqlx::query("SELECT * FROM sessions WHERE account_id = ? AND id = ?")
        .bind(account_id.as_str())
        .bind(id.as_str())
        .fetch_optional(&mut *conn)
        .await?;
    row.as_ref()
        .map(super::rows::session)
        .transpose()
        .map_err(Into::into)
}

pub async fn get_projection(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    id: &SessionId,
) -> Result<Option<SessionProjection>, InfraError> {
    let Some(session) = get(conn, account_id, id).await? else {
        return Ok(None);
    };
    Ok(Some(enrich_item(conn, session).await?))
}

pub async fn get_private_by_address(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    address: &EmailAddress,
) -> Result<Option<Session>, InfraError> {
    let row = sqlx::query(
        "SELECT * FROM sessions WHERE account_id = ? AND kind = 'private' AND normalized_address = ?",
    )
    .bind(account_id.as_str())
    .bind(address.as_str())
    .fetch_optional(&mut *conn)
    .await?;
    row.as_ref()
        .map(super::rows::session)
        .transpose()
        .map_err(Into::into)
}

pub async fn get_group_by_group_id(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    group_id: &GroupId,
) -> Result<Option<Session>, InfraError> {
    let row = sqlx::query(
        "SELECT * FROM sessions WHERE account_id = ? AND kind = 'group' AND group_id = ?",
    )
    .bind(account_id.as_str())
    .bind(group_id.as_str())
    .fetch_optional(&mut *conn)
    .await?;
    row.as_ref()
        .map(super::rows::session)
        .transpose()
        .map_err(Into::into)
}

/// 沿 merged_into_id 解析到存活根会话
pub async fn resolve_root(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    id: &SessionId,
) -> Result<Option<Session>, InfraError> {
    let mut current = id.clone();
    for _ in 0..32 {
        let Some(session) = get(conn, account_id, &current).await? else {
            return Ok(None);
        };
        match session.merged_into_id {
            Some(next) => current = next,
            None => return Ok(Some(session)),
        }
    }
    Err(InfraError::Conflict {
        message: "会话合并链过深或成环".into(),
    })
}

pub async fn list(
    conn: &mut SqliteConnection,
    account_id: Option<&AccountId>,
    cursor: Option<&str>,
    limit: u32,
    kind: Option<SessionKind>,
) -> Result<CursorPage<SessionProjection>, InfraError> {
    let kind = kind.map(|k| k.as_str());
    if limit == 0 || limit > 500 {
        return Err(InfraError::Conflict {
            message: "分页大小须为 1..500".into(),
        });
    }
    let (cursor_at, mut cursor_account, cursor_id) = parse_session_cursor(cursor)?;
    let account_ids = accounts::resolve_list_account_ids(conn, account_id).await?;
    if account_ids.is_empty() {
        return Ok(CursorPage::empty());
    }
    if cursor_account.is_none() && account_ids.len() == 1 {
        cursor_account = Some(account_ids[0].as_str().to_owned());
    }

    let mut qb = QueryBuilder::new("SELECT * FROM sessions WHERE ");
    accounts::push_account_id_in(&mut qb, &account_ids);
    qb.push(" AND merged_into_id IS NULL AND hidden_at IS NULL AND (");
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
    qb.push_bind(i64::from(limit) + 1);

    let rows = qb.build().fetch_all(&mut *conn).await?;
    let has_more = rows.len() > limit as usize;
    let mut sessions = rows
        .iter()
        .take(limit as usize)
        .map(super::rows::session)
        .collect::<Result<Vec<_>, _>>()?;
    let next_cursor = if has_more {
        sessions.last().map(|s| {
            format!(
                "{}:{}:{}",
                s.last_message_at.unwrap_or(-1),
                s.account_id.as_str(),
                s.id.as_str()
            )
        })
    } else {
        None
    };
    let mut items = Vec::with_capacity(sessions.len());
    for session in sessions.drain(..) {
        items.push(enrich_item(conn, session).await?);
    }
    Ok(CursorPage {
        items,
        next_cursor,
        has_more,
    })
}

fn parse_session_cursor(
    cursor: Option<&str>,
) -> Result<(Option<i64>, Option<String>, Option<String>), InfraError> {
    let Some(raw) = cursor else {
        return Ok((None, None, None));
    };
    let parts: Vec<&str> = raw.splitn(3, ':').collect();
    match parts.as_slice() {
        [at, account_id, id] => {
            let at: i64 = at.parse().map_err(|_| InfraError::Conflict {
                message: "无效会话游标时间".into(),
            })?;
            Ok((
                Some(at),
                Some((*account_id).to_owned()),
                Some((*id).to_owned()),
            ))
        }
        [at, id] => {
            let at: i64 = at.parse().map_err(|_| InfraError::Conflict {
                message: "无效会话游标时间".into(),
            })?;
            Ok((Some(at), None, Some((*id).to_owned())))
        }
        _ => Err(InfraError::Conflict {
            message: "无效会话游标".into(),
        }),
    }
}

async fn enrich_item(
    conn: &mut SqliteConnection,
    session: Session,
) -> Result<SessionProjection, InfraError> {
    let contact_display_name = if let Some(ref cid) = session.contact_id {
        sqlx::query_scalar::<_, String>(
            "SELECT display_name FROM contacts WHERE account_id = ? AND id = ?",
        )
        .bind(session.account_id.as_str())
        .bind(cid.as_str())
        .fetch_optional(&mut *conn)
        .await?
        .filter(|s| !s.is_empty())
    } else {
        None
    };
    let group_name = if let Some(ref gid) = session.group_id {
        let row =
            sqlx::query("SELECT name, name_is_manual FROM groups WHERE account_id = ? AND id = ?")
                .bind(session.account_id.as_str())
                .bind(gid.as_str())
                .fetch_optional(&mut *conn)
                .await?;
        match row {
            Some(row) => {
                use sqlx::Row;
                let stored: String = row.try_get("name")?;
                let manual: i64 = row.try_get("name_is_manual")?;
                if manual != 0 {
                    Some(stored)
                } else {
                    // 自动群：列表展示优先用成员/联系人昵称拼标题
                    Some(
                        super::groups::resolve_auto_title(&mut *conn, &session.account_id, gid)
                            .await?,
                    )
                }
            }
            None => None,
        }
    } else {
        None
    };
    let last_message = if let Some(ref mid) = session.last_message_id {
        let row = sqlx::query(
            "SELECT * FROM messages WHERE account_id = ? AND id = ? AND sync_status != 'deleted_remote'",
        )
        .bind(session.account_id.as_str())
        .bind(mid.as_str())
        .fetch_optional(&mut *conn)
        .await?;
        match row {
            Some(row) => {
                let m = super::rows::message(&row)?;
                Some(m)
            }
            None => None,
        }
    } else {
        None
    };
    Ok(SessionProjection {
        session,
        last_message,
        contact_display_name,
        group_name,
    })
}

pub async fn upsert(conn: &mut SqliteConnection, session: &Session) -> Result<(), InfraError> {
    sqlx::query(
        "INSERT INTO sessions (
            id, account_id, kind, normalized_address, contact_id, group_id, merged_into_id,
            last_message_at, last_message_id, unread_count, hidden_at, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(account_id, id) DO UPDATE SET
            kind=excluded.kind,
            normalized_address=excluded.normalized_address,
            contact_id=excluded.contact_id,
            group_id=excluded.group_id,
            merged_into_id=excluded.merged_into_id,
            last_message_at=excluded.last_message_at,
            last_message_id=excluded.last_message_id,
            unread_count=excluded.unread_count,
            updated_at=excluded.updated_at",
        // 冲突更新不改 hidden_at，避免投影重写冲掉用户隐藏
    )
    .bind(session.id.as_str())
    .bind(session.account_id.as_str())
    .bind(session.kind.as_str())
    .bind(session.normalized_address.as_ref().map(|a| a.as_str()))
    .bind(session.contact_id.as_ref().map(|c| c.as_str()))
    .bind(session.group_id.as_ref().map(|g| g.as_str()))
    .bind(session.merged_into_id.as_ref().map(|s| s.as_str()))
    .bind(session.last_message_at)
    .bind(session.last_message_id.as_ref().map(|m| m.as_str()))
    .bind(session.unread_count)
    .bind(session.hidden_at)
    .bind(session.created_at)
    .bind(session.updated_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn set_hidden_at(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    session_id: &SessionId,
    hidden_at: Option<i64>,
    now: i64,
) -> Result<(), InfraError> {
    let result = sqlx::query(
        "UPDATE sessions
         SET hidden_at = ?, updated_at = ?
         WHERE account_id = ? AND id = ?",
    )
    .bind(hidden_at)
    .bind(now)
    .bind(account_id.as_str())
    .bind(session_id.as_str())
    .execute(&mut *conn)
    .await?;
    if result.rows_affected() == 0 {
        return Err(InfraError::Conflict {
            message: format!("会话不存在: {session_id}"),
        });
    }
    Ok(())
}

/// source.merged_into_id = target（不做消息搬迁）
pub async fn merge_into(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    source_id: &SessionId,
    target_id: &SessionId,
    now: i64,
) -> Result<(), InfraError> {
    if source_id == target_id {
        return Err(InfraError::Conflict {
            message: "不能合并到自身".into(),
        });
    }
    let result = sqlx::query(
        "UPDATE sessions SET merged_into_id = ?, updated_at = ?
         WHERE account_id = ? AND id = ?",
    )
    .bind(target_id.as_str())
    .bind(now)
    .bind(account_id.as_str())
    .bind(source_id.as_str())
    .execute(&mut *conn)
    .await?;
    if result.rows_affected() == 0 {
        return Err(InfraError::Conflict {
            message: format!("源会话不存在: {source_id}"),
        });
    }
    Ok(())
}

pub async fn clear_contact_id_refs(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    contact_id: &ContactId,
) -> Result<(), InfraError> {
    sqlx::query(
        "UPDATE sessions SET contact_id = NULL, updated_at = ?
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

pub async fn list_messages(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    session_id: &SessionId,
    cursor: Option<&str>,
    limit: u32,
    direction: PageDirection,
) -> Result<CursorPage<MessageWithRecipients>, InfraError> {
    if limit == 0 || limit > 500 {
        return Err(InfraError::Conflict {
            message: "分页大小须为 1..500".into(),
        });
    }
    let (cursor_at, cursor_id) = parse_msg_cursor(cursor)?;
    // 含 deleted_remote，由前端过滤
    let rows = match direction {
        PageDirection::Older => {
            sqlx::query(
                "SELECT * FROM messages
                 WHERE account_id = ? AND session_id = ?
                   AND (
                     ? IS NULL
                     OR COALESCE(received_at, sent_at, 0) < ?
                     OR (COALESCE(received_at, sent_at, 0) = ? AND id < ?)
                   )
                 ORDER BY COALESCE(received_at, sent_at, 0) DESC, id DESC
                 LIMIT ?",
            )
            .bind(account_id.as_str())
            .bind(session_id.as_str())
            .bind(cursor_id.as_deref())
            .bind(cursor_at.unwrap_or(i64::MAX))
            .bind(cursor_at.unwrap_or(i64::MAX))
            .bind(cursor_id.as_deref())
            .bind(i64::from(limit) + 1)
            .fetch_all(&mut *conn)
            .await?
        }
        PageDirection::Newer => {
            sqlx::query(
                "SELECT * FROM messages
                 WHERE account_id = ? AND session_id = ?
                   AND (
                     ? IS NULL
                     OR COALESCE(received_at, sent_at, 0) > ?
                     OR (COALESCE(received_at, sent_at, 0) = ? AND id > ?)
                   )
                 ORDER BY COALESCE(received_at, sent_at, 0) ASC, id ASC
                 LIMIT ?",
            )
            .bind(account_id.as_str())
            .bind(session_id.as_str())
            .bind(cursor_id.as_deref())
            .bind(cursor_at.unwrap_or(-1))
            .bind(cursor_at.unwrap_or(-1))
            .bind(cursor_id.as_deref())
            .bind(i64::from(limit) + 1)
            .fetch_all(&mut *conn)
            .await?
        }
    };

    let has_more = rows.len() > limit as usize;
    let message_rows: Vec<_> = rows.into_iter().take(limit as usize).collect();
    let mut items = Vec::with_capacity(message_rows.len());
    for row in &message_rows {
        let message = super::rows::message(row)?;
        let recipients = sqlx::query(
            "SELECT * FROM message_recipients WHERE account_id = ? AND message_id = ? ORDER BY role, seq",
        )
        .bind(account_id.as_str())
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

fn parse_msg_cursor(cursor: Option<&str>) -> Result<(Option<i64>, Option<String>), InfraError> {
    let Some(raw) = cursor else {
        return Ok((None, None));
    };
    let (at, id) = raw.split_once(':').ok_or_else(|| InfraError::Conflict {
        message: "无效消息游标".into(),
    })?;
    let at: i64 = at.parse().map_err(|_| InfraError::Conflict {
        message: "无效消息游标时间".into(),
    })?;
    Ok((Some(at), Some(id.to_owned())))
}

pub async fn recompute_projection(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    session_id: &SessionId,
) -> Result<Session, InfraError> {
    let last = sqlx::query(
        "SELECT id, COALESCE(received_at, sent_at) AS at
         FROM messages
         WHERE account_id = ? AND session_id = ?
           AND sync_status != 'deleted_remote'
         ORDER BY COALESCE(received_at, sent_at, 0) DESC, id DESC
         LIMIT 1",
    )
    .bind(account_id.as_str())
    .bind(session_id.as_str())
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
        "SELECT COUNT(*) FROM messages
         WHERE account_id = ? AND session_id = ?
           AND sync_status != 'deleted_remote' AND is_read = 0",
    )
    .bind(account_id.as_str())
    .bind(session_id.as_str())
    .fetch_one(&mut *conn)
    .await?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    // 有比隐藏时刻更新的消息 → 自动取消隐藏
    sqlx::query(
        "UPDATE sessions
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
    .bind(session_id.as_str())
    .execute(&mut *conn)
    .await?;

    get(conn, account_id, session_id)
        .await?
        .ok_or_else(|| InfraError::Conflict {
            message: format!("会话不存在: {session_id}"),
        })
}

/// 将指定消息的 session_id 改为目标；返回受影响的旧 session_id 集合
pub async fn reassign_messages(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_ids: &[MessageId],
    target_session_id: &SessionId,
) -> Result<Vec<SessionId>, InfraError> {
    let mut old_ids = Vec::new();
    for mid in message_ids {
        let old: Option<String> =
            sqlx::query_scalar("SELECT session_id FROM messages WHERE account_id = ? AND id = ?")
                .bind(account_id.as_str())
                .bind(mid.as_str())
                .fetch_optional(&mut *conn)
                .await?;
        let Some(old) = old else {
            continue;
        };
        if old != target_session_id.as_str() {
            old_ids.push(SessionId::new(old));
            sqlx::query(
                "UPDATE messages SET session_id = ?, local_updated_at = ?
                 WHERE account_id = ? AND id = ?",
            )
            .bind(target_session_id.as_str())
            .bind(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0),
            )
            .bind(account_id.as_str())
            .bind(mid.as_str())
            .execute(&mut *conn)
            .await?;
        }
    }
    old_ids.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    old_ids.dedup();
    Ok(old_ids)
}

/// 将某 session 下全部消息迁到目标 session
pub async fn move_all_messages(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    source_session_id: &SessionId,
    target_session_id: &SessionId,
) -> Result<u64, InfraError> {
    if source_session_id == target_session_id {
        return Ok(0);
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let result = sqlx::query(
        "UPDATE messages SET session_id = ?, local_updated_at = ?
         WHERE account_id = ? AND session_id = ?",
    )
    .bind(target_session_id.as_str())
    .bind(now)
    .bind(account_id.as_str())
    .bind(source_session_id.as_str())
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected())
}

/// 便捷：创建私聊 Session 骨架（调用方填 contact_id 等）
pub fn new_private(
    account_id: AccountId,
    address: EmailAddress,
    contact_id: Option<ContactId>,
    now: i64,
) -> Session {
    Session {
        id: SessionId::new(format!("sess-{}", uuid::Uuid::new_v4())),
        account_id,
        kind: SessionKind::Private,
        normalized_address: Some(address),
        contact_id,
        group_id: None,
        merged_into_id: None,
        last_message_at: None,
        last_message_id: None,
        unread_count: 0,
        hidden_at: None,
        created_at: now,
        updated_at: now,
    }
}

pub fn new_group(account_id: AccountId, group_id: GroupId, now: i64) -> Session {
    Session {
        id: SessionId::new(format!("sess-{}", uuid::Uuid::new_v4())),
        account_id,
        kind: SessionKind::Group,
        normalized_address: None,
        contact_id: None,
        group_id: Some(group_id),
        merged_into_id: None,
        last_message_at: None,
        last_message_id: None,
        unread_count: 0,
        hidden_at: None,
        created_at: now,
        updated_at: now,
    }
}

/// 查询联表数据；展示 DTO 由 application 组装。
pub struct SessionProjection {
    pub session: Session,
    pub contact_display_name: Option<String>,
    pub group_name: Option<String>,
    pub last_message: Option<crate::domain::entities::Message>,
}
