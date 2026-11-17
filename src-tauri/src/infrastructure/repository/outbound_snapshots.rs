//! 外发快照持久化：不可变 MIME/envelope，供重试复用。

use crate::domain::value_objects::{AccountId, TimestampMs};
use crate::infrastructure::error::InfraError;
use sqlx::{Row, SqliteConnection};

/// 快照行
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundSnapshotRow {
    pub snapshot_id: String,
    pub account_id: AccountId,
    pub content_version: i64,
    pub internet_message_id: String,
    pub date: String,
    /// 存放完整 MIME 字节的受管理内容引用
    pub mime_content_id: String,
    pub envelope_json: String,
    pub subject: String,
    pub text_body: String,
    pub html_body: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    pub created_at: TimestampMs,
}

/// 快照附件引用
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotAttachmentRow {
    pub content_id: String,
    pub name: String,
    pub mime: String,
    pub sort_order: i32,
}

pub async fn insert_snapshot(
    conn: &mut SqliteConnection,
    row: &OutboundSnapshotRow,
    attachments: &[SnapshotAttachmentRow],
) -> Result<(), InfraError> {
    sqlx::query(
        "INSERT INTO outbound_snapshots
         (snapshot_id, account_id, content_version, internet_message_id, date, mime_content_id,
          envelope_json, subject, text_body, html_body, in_reply_to, references_header, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&row.snapshot_id)
    .bind(row.account_id.as_str())
    .bind(row.content_version)
    .bind(&row.internet_message_id)
    .bind(&row.date)
    .bind(&row.mime_content_id)
    .bind(&row.envelope_json)
    .bind(&row.subject)
    .bind(&row.text_body)
    .bind(&row.html_body)
    .bind(&row.in_reply_to)
    .bind(&row.references)
    .bind(row.created_at)
    .execute(&mut *conn)
    .await?;

    for att in attachments {
        sqlx::query(
            "INSERT INTO snapshot_attachments
             (account_id, snapshot_id, content_id, name, mime, sort_order)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(row.account_id.as_str())
        .bind(&row.snapshot_id)
        .bind(&att.content_id)
        .bind(&att.name)
        .bind(&att.mime)
        .bind(att.sort_order)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

pub async fn get_snapshot(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    snapshot_id: &str,
) -> Result<Option<OutboundSnapshotRow>, InfraError> {
    let row = sqlx::query(
        "SELECT snapshot_id, account_id, content_version, internet_message_id, date,
                mime_content_id, envelope_json, subject, text_body, html_body,
                in_reply_to, references_header, created_at
         FROM outbound_snapshots WHERE account_id = ? AND snapshot_id = ?",
    )
    .bind(account_id.as_str())
    .bind(snapshot_id)
    .fetch_optional(&mut *conn)
    .await?;
    row.map(|r| {
        Ok(OutboundSnapshotRow {
            snapshot_id: r.try_get("snapshot_id")?,
            account_id: AccountId::new(r.try_get::<String, _>("account_id")?),
            content_version: r.try_get("content_version")?,
            internet_message_id: r.try_get("internet_message_id")?,
            date: r.try_get("date")?,
            mime_content_id: r.try_get("mime_content_id")?,
            envelope_json: r.try_get("envelope_json")?,
            subject: r.try_get("subject")?,
            text_body: r.try_get("text_body")?,
            html_body: r.try_get("html_body")?,
            in_reply_to: r.try_get("in_reply_to")?,
            references: r.try_get("references_header")?,
            created_at: r.try_get("created_at")?,
        })
    })
    .transpose()
}

pub async fn list_snapshot_attachments(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    snapshot_id: &str,
) -> Result<Vec<SnapshotAttachmentRow>, InfraError> {
    let rows = sqlx::query(
        "SELECT content_id, name, mime, sort_order FROM snapshot_attachments
         WHERE account_id = ? AND snapshot_id = ? ORDER BY sort_order, content_id",
    )
    .bind(account_id.as_str())
    .bind(snapshot_id)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            Ok(SnapshotAttachmentRow {
                content_id: r.try_get("content_id")?,
                name: r.try_get("name")?,
                mime: r.try_get("mime")?,
                sort_order: r.try_get("sort_order")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?)
}
