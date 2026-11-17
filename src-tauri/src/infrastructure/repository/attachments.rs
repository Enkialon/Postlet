//! 附件内容与引用仓储；连接/事务由调用方持有。

use crate::domain::value_objects::{AccountId, MessageId, SessionId, TimestampMs};
use crate::infrastructure::error::InfraError;
use sqlx::{Row, SqliteConnection};

/// 已落库的附件内容行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentContentRow {
    pub content_id: String,
    pub account_id: AccountId,
    pub storage_key: String,
    pub byte_length: i64,
    pub digest: String,
    pub created_at: TimestampMs,
}

/// 草稿正文行（收件人仍为 JSON 文本）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposeDraftRow {
    pub draft_id: String,
    pub account_id: AccountId,
    pub session_id: SessionId,
    pub reply_to_message_id: Option<String>,
    pub to_json: String,
    pub cc_json: String,
    pub bcc_json: String,
    pub subject: String,
    pub body_html: String,
    pub updated_at: TimestampMs,
}

/// 草稿/消息附件引用（含内容长度，便于组装 DTO）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentRefRow {
    pub content_id: String,
    pub name: String,
    pub mime: String,
    pub sort_order: i32,
    pub byte_length: i64,
}

pub async fn insert_content(
    conn: &mut SqliteConnection,
    row: &AttachmentContentRow,
) -> Result<(), InfraError> {
    sqlx::query(
        "INSERT INTO attachment_contents
         (content_id, account_id, storage_key, byte_length, digest, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&row.content_id)
    .bind(row.account_id.as_str())
    .bind(&row.storage_key)
    .bind(row.byte_length)
    .bind(&row.digest)
    .bind(row.created_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn get_content(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    content_id: &str,
) -> Result<Option<AttachmentContentRow>, InfraError> {
    let row = sqlx::query(
        "SELECT content_id, account_id, storage_key, byte_length, digest, created_at
         FROM attachment_contents WHERE account_id = ? AND content_id = ?",
    )
    .bind(account_id.as_str())
    .bind(content_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(map_content_row))
}

pub async fn delete_content(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    content_id: &str,
) -> Result<(), InfraError> {
    sqlx::query("DELETE FROM attachment_contents WHERE account_id = ? AND content_id = ?")
        .bind(account_id.as_str())
        .bind(content_id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

pub async fn list_contents(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
) -> Result<Vec<AttachmentContentRow>, InfraError> {
    let rows = sqlx::query(
        "SELECT content_id, account_id, storage_key, byte_length, digest, created_at
         FROM attachment_contents WHERE account_id = ?",
    )
    .bind(account_id.as_str())
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows.into_iter().map(map_content_row).collect())
}

/// 无任何草稿/消息引用的内容行（发送/归档引用表由后续任务接入）。
pub async fn collect_unreferenced(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
) -> Result<Vec<AttachmentContentRow>, InfraError> {
    let rows = sqlx::query(
        "SELECT c.content_id, c.account_id, c.storage_key, c.byte_length, c.digest, c.created_at
         FROM attachment_contents c
         WHERE c.account_id = ?
           AND NOT EXISTS (
             SELECT 1 FROM draft_attachments d
             WHERE d.account_id = c.account_id AND d.content_id = c.content_id
           )
           AND NOT EXISTS (
             SELECT 1 FROM message_attachments m
             WHERE m.account_id = c.account_id AND m.content_id = c.content_id
           )",
    )
    .bind(account_id.as_str())
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows.into_iter().map(map_content_row).collect())
}

pub async fn get_draft_by_session(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    session_id: &SessionId,
) -> Result<Option<ComposeDraftRow>, InfraError> {
    let row = sqlx::query(
        "SELECT draft_id, account_id, session_id, reply_to_message_id, to_json, cc_json, bcc_json,
                subject, body_html, updated_at
         FROM compose_drafts WHERE account_id = ? AND session_id = ?",
    )
    .bind(account_id.as_str())
    .bind(session_id.as_str())
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(map_draft_row))
}

pub async fn upsert_draft(conn: &mut SqliteConnection, row: &ComposeDraftRow) -> Result<(), InfraError> {
    sqlx::query(
        "INSERT INTO compose_drafts
         (draft_id, account_id, session_id, reply_to_message_id, to_json, cc_json, bcc_json,
          subject, body_html, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(account_id, session_id) DO UPDATE SET
           reply_to_message_id = excluded.reply_to_message_id,
           to_json = excluded.to_json,
           cc_json = excluded.cc_json,
           bcc_json = excluded.bcc_json,
           subject = excluded.subject,
           body_html = excluded.body_html,
           updated_at = excluded.updated_at",
    )
    .bind(&row.draft_id)
    .bind(row.account_id.as_str())
    .bind(row.session_id.as_str())
    .bind(&row.reply_to_message_id)
    .bind(&row.to_json)
    .bind(&row.cc_json)
    .bind(&row.bcc_json)
    .bind(&row.subject)
    .bind(&row.body_html)
    .bind(row.updated_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// 确保会话草稿存在；返回 draft_id（已有则复用）。
pub async fn ensure_draft(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    session_id: &SessionId,
    now: TimestampMs,
) -> Result<String, InfraError> {
    if let Some(existing) = get_draft_by_session(conn, account_id, session_id).await? {
        return Ok(existing.draft_id);
    }
    let draft_id = uuid::Uuid::new_v4().to_string();
    upsert_draft(
        conn,
        &ComposeDraftRow {
            draft_id: draft_id.clone(),
            account_id: account_id.clone(),
            session_id: session_id.clone(),
            reply_to_message_id: None,
            to_json: "[]".into(),
            cc_json: "[]".into(),
            bcc_json: "[]".into(),
            subject: String::new(),
            body_html: String::new(),
            updated_at: now,
        },
    )
    .await?;
    Ok(draft_id)
}

pub async fn delete_draft(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    session_id: &SessionId,
) -> Result<bool, InfraError> {
    let res = sqlx::query("DELETE FROM compose_drafts WHERE account_id = ? AND session_id = ?")
        .bind(account_id.as_str())
        .bind(session_id.as_str())
        .execute(&mut *conn)
        .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_drafts_for_account(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
) -> Result<(), InfraError> {
    sqlx::query("DELETE FROM compose_drafts WHERE account_id = ?")
        .bind(account_id.as_str())
        .execute(&mut *conn)
        .await?;
    Ok(())
}

pub async fn delete_contents_for_account(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
) -> Result<(), InfraError> {
    sqlx::query("DELETE FROM attachment_contents WHERE account_id = ?")
        .bind(account_id.as_str())
        .execute(&mut *conn)
        .await?;
    Ok(())
}

pub async fn list_draft_attachments(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    draft_id: &str,
) -> Result<Vec<AttachmentRefRow>, InfraError> {
    let rows = sqlx::query(
        "SELECT d.content_id, d.name, d.mime, d.sort_order, c.byte_length
         FROM draft_attachments d
         JOIN attachment_contents c
           ON c.account_id = d.account_id AND c.content_id = d.content_id
         WHERE d.account_id = ? AND d.draft_id = ?
         ORDER BY d.sort_order ASC, d.content_id ASC",
    )
    .bind(account_id.as_str())
    .bind(draft_id)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows.into_iter().map(map_ref_row).collect())
}

pub async fn replace_draft_attachments(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    draft_id: &str,
    attachments: &[AttachmentRefRow],
) -> Result<(), InfraError> {
    sqlx::query("DELETE FROM draft_attachments WHERE account_id = ? AND draft_id = ?")
        .bind(account_id.as_str())
        .bind(draft_id)
        .execute(&mut *conn)
        .await?;
    for att in attachments {
        if get_content(conn, account_id, &att.content_id).await?.is_none() {
            return Err(InfraError::Conflict {
                message: format!("附件内容不存在: {}", att.content_id),
            });
        }
        sqlx::query(
            "INSERT INTO draft_attachments
             (account_id, draft_id, content_id, name, mime, sort_order)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(account_id.as_str())
        .bind(draft_id)
        .bind(&att.content_id)
        .bind(&att.name)
        .bind(&att.mime)
        .bind(att.sort_order)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

pub async fn insert_draft_attachment(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    draft_id: &str,
    att: &AttachmentRefRow,
) -> Result<(), InfraError> {
    sqlx::query(
        "INSERT INTO draft_attachments
         (account_id, draft_id, content_id, name, mime, sort_order)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(account_id.as_str())
    .bind(draft_id)
    .bind(&att.content_id)
    .bind(&att.name)
    .bind(&att.mime)
    .bind(att.sort_order)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn next_draft_attachment_order(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    draft_id: &str,
) -> Result<i32, InfraError> {
    let row = sqlx::query(
        "SELECT COALESCE(MAX(sort_order) + 1, 0) AS next_order
         FROM draft_attachments WHERE account_id = ? AND draft_id = ?",
    )
    .bind(account_id.as_str())
    .bind(draft_id)
    .fetch_one(&mut *conn)
    .await?;
    Ok(row.get::<i32, _>("next_order"))
}

pub async fn insert_message_attachment(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_id: &MessageId,
    att: &AttachmentRefRow,
) -> Result<(), InfraError> {
    sqlx::query(
        "INSERT INTO message_attachments
         (account_id, message_id, content_id, name, mime, sort_order)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(account_id.as_str())
    .bind(message_id.as_str())
    .bind(&att.content_id)
    .bind(&att.name)
    .bind(&att.mime)
    .bind(att.sort_order)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn list_message_attachments(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Vec<AttachmentRefRow>, InfraError> {
    let rows = sqlx::query(
        "SELECT m.content_id, m.name, m.mime, m.sort_order, c.byte_length
         FROM message_attachments m
         JOIN attachment_contents c
           ON c.account_id = m.account_id AND c.content_id = m.content_id
         WHERE m.account_id = ? AND m.message_id = ?
         ORDER BY m.sort_order ASC, m.content_id ASC",
    )
    .bind(account_id.as_str())
    .bind(message_id.as_str())
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows.into_iter().map(map_ref_row).collect())
}

fn map_content_row(r: sqlx::sqlite::SqliteRow) -> AttachmentContentRow {
    AttachmentContentRow {
        content_id: r.get("content_id"),
        account_id: AccountId::new(r.get::<String, _>("account_id")),
        storage_key: r.get("storage_key"),
        byte_length: r.get("byte_length"),
        digest: r.get("digest"),
        created_at: r.get("created_at"),
    }
}

fn map_draft_row(r: sqlx::sqlite::SqliteRow) -> ComposeDraftRow {
    ComposeDraftRow {
        draft_id: r.get("draft_id"),
        account_id: AccountId::new(r.get::<String, _>("account_id")),
        session_id: SessionId::new(r.get::<String, _>("session_id")),
        reply_to_message_id: r.get("reply_to_message_id"),
        to_json: r.get("to_json"),
        cc_json: r.get("cc_json"),
        bcc_json: r.get("bcc_json"),
        subject: r.get("subject"),
        body_html: r.get("body_html"),
        updated_at: r.get("updated_at"),
    }
}

fn map_ref_row(r: sqlx::sqlite::SqliteRow) -> AttachmentRefRow {
    AttachmentRefRow {
        content_id: r.get("content_id"),
        name: r.get("name"),
        mime: r.get("mime"),
        sort_order: r.get("sort_order"),
        byte_length: r.get("byte_length"),
    }
}
