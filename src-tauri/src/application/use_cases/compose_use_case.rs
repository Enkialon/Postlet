//! 本地写信用例：SQLite 草稿 + 受管理附件引用；发送仍拒绝附件直至 Task 08。

use crate::application::dto::{
    ComposeAttachmentDraft, ComposeDraft, ComposeRecipientRef, ImportAttachmentCommand,
    SaveComposeDraftCommand, SendComposedMailCommand,
};
use crate::application::error::{AppError, AppErrorCode};
use crate::application::use_cases::attachment_use_case;
use crate::domain::entities::{Message, MessageRecipient, MessageWithRecipients};
use crate::domain::value_objects::{
    AccountId, BodyContentType, EmailAddress, Importance, MessageId, MessageSyncStatus,
    RecipientRole, SessionId, TimestampMs,
};
use crate::infrastructure::attachment_store;
use crate::infrastructure::mail::outbound_content::{
    build_snapshot, build_references, new_internet_message_id, SnapshotAttachmentBytes,
    SnapshotInput, SnapshotRecipient,
};
use crate::infrastructure::paths::AppPaths;
use crate::infrastructure::repository::{
    accounts, attachments, messages, outbound_snapshots, sessions,
};
use sqlx::SqlitePool;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

static ID_SEQ: AtomicU64 = AtomicU64::new(1);

fn now_ms() -> TimestampMs {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn next_local_id(prefix: &str) -> String {
    let n = ID_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{}-{n}", now_ms())
}

fn strip_html(html: &str) -> String {
    let mut out = html
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("</p>", "\n");
    while let Some(start) = out.find('<') {
        if let Some(end_rel) = out[start..].find('>') {
            out.replace_range(start..start + end_rel + 1, "");
        } else {
            break;
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn reply_subject(original: &str) -> String {
    let t = original.trim();
    if t.is_empty() {
        return String::new();
    }
    if t.len() >= 3 && t[..3].eq_ignore_ascii_case("re:") {
        return t.to_string();
    }
    format!("Re: {t}")
}

fn normalize_recipients(list: &[ComposeRecipientRef]) -> Vec<ComposeRecipientRef> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for raw in list {
        let Some(addr) = EmailAddress::parse(raw.address.as_str()) else {
            continue;
        };
        let key = addr.as_str().to_string();
        if !seen.insert(key) {
            continue;
        }
        let display = raw.display_name.trim();
        out.push(ComposeRecipientRef {
            display_name: if display.is_empty() {
                addr.as_str().to_string()
            } else {
                display.to_string()
            },
            address: addr,
        });
    }
    out
}

fn json_recipients(list: &[ComposeRecipientRef]) -> Result<String, AppError> {
    serde_json::to_string(list).map_err(|_| AppError::internal("序列化草稿收件人失败"))
}

fn parse_recipients(raw: &str) -> Result<Vec<ComposeRecipientRef>, AppError> {
    serde_json::from_str(raw).map_err(|_| AppError::internal("解析草稿收件人失败"))
}

fn refs_from_draft_attachments(list: &[ComposeAttachmentDraft]) -> Vec<attachments::AttachmentRefRow> {
    list.iter()
        .enumerate()
        .map(|(i, a)| attachments::AttachmentRefRow {
            content_id: a.content_id.clone(),
            name: a.name.clone(),
            mime: a.mime.clone(),
            sort_order: i as i32,
            byte_length: a.size,
        })
        .collect()
}

fn draft_from_row(
    row: attachments::ComposeDraftRow,
    atts: Vec<attachments::AttachmentRefRow>,
) -> Result<ComposeDraft, AppError> {
    Ok(ComposeDraft {
        account_id: row.account_id,
        session_id: row.session_id,
        reply_to_message_id: row.reply_to_message_id.map(MessageId::new),
        to: parse_recipients(&row.to_json)?,
        cc: parse_recipients(&row.cc_json)?,
        bcc: parse_recipients(&row.bcc_json)?,
        subject: row.subject,
        body_html: row.body_html,
        attachments: atts
            .into_iter()
            .map(|a| ComposeAttachmentDraft {
                content_id: a.content_id,
                name: a.name,
                size: a.byte_length,
                mime: a.mime,
            })
            .collect(),
        updated_at: row.updated_at,
    })
}

/// 读取会话草稿；无则 `None`
pub async fn get_draft(
    db: &SqlitePool,
    account_id: &AccountId,
    session_id: &SessionId,
) -> Result<Option<ComposeDraft>, AppError> {
    let mut conn = db.acquire().await?;
    let Some(row) = attachments::get_draft_by_session(&mut conn, account_id, session_id).await?
    else {
        return Ok(None);
    };
    let atts = attachments::list_draft_attachments(&mut conn, account_id, &row.draft_id).await?;
    Ok(Some(draft_from_row(row, atts)?))
}

/// 保存会话草稿（SQLite + 附件引用）；持附件锁覆盖引用变更与回收。
pub async fn save_draft(
    db: &SqlitePool,
    paths: &AppPaths,
    cmd: SaveComposeDraftCommand,
) -> Result<ComposeDraft, AppError> {
    save_draft_with_refs(db, paths, cmd).await
}

pub async fn save_draft_with_refs(
    db: &SqlitePool,
    paths: &crate::infrastructure::paths::AppPaths,
    cmd: SaveComposeDraftCommand,
) -> Result<ComposeDraft, AppError> {
    {
        let mut conn = db.acquire().await?;
        accounts::get(&mut conn, &cmd.account_id)
            .await?
            .ok_or_else(|| AppError::not_found(format!("账户不存在: {}", cmd.account_id)))?;
    }

    let _lock = attachment_store::lock_account(&paths.attachments_dir, &cmd.account_id).map_err(
        |err| match err {
            crate::infrastructure::error::InfraError::Conflict { message } => AppError {
                code: crate::application::error::AppErrorCode::Conflict,
                message,
                retryable: true,
            },
            other => other.into(),
        },
    )?;

    let now = now_ms();
    let draft = ComposeDraft {
        account_id: cmd.account_id.clone(),
        session_id: cmd.session_id.clone(),
        reply_to_message_id: cmd.reply_to_message_id,
        to: normalize_recipients(&cmd.to),
        cc: normalize_recipients(&cmd.cc),
        bcc: normalize_recipients(&cmd.bcc),
        subject: cmd.subject,
        body_html: cmd.body_html,
        attachments: cmd.attachments,
        updated_at: now,
    };

    let mut tx = db.begin().await?;
    let existing =
        attachments::get_draft_by_session(&mut tx, &draft.account_id, &draft.session_id).await?;
    let draft_id = existing
        .map(|r| r.draft_id)
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    attachments::upsert_draft(
        &mut tx,
        &attachments::ComposeDraftRow {
            draft_id: draft_id.clone(),
            account_id: draft.account_id.clone(),
            session_id: draft.session_id.clone(),
            reply_to_message_id: draft
                .reply_to_message_id
                .as_ref()
                .map(|m| m.as_str().to_string()),
            to_json: json_recipients(&draft.to)?,
            cc_json: json_recipients(&draft.cc)?,
            bcc_json: json_recipients(&draft.bcc)?,
            subject: draft.subject.clone(),
            body_html: draft.body_html.clone(),
            updated_at: draft.updated_at,
        },
    )
    .await?;

    // ON CONFLICT 不更新 draft_id；重新读取权威 draft_id
    let draft_id = attachments::get_draft_by_session(&mut tx, &draft.account_id, &draft.session_id)
        .await?
        .map(|r| r.draft_id)
        .unwrap_or(draft_id);

    attachments::replace_draft_attachments(
        &mut tx,
        &draft.account_id,
        &draft_id,
        &refs_from_draft_attachments(&draft.attachments),
    )
    .await?;
    accounts::bump_change_version(&mut tx, &draft.account_id).await?;
    tx.commit().await?;

    attachment_use_case::recycle_unreferenced(db, paths, &draft.account_id, Some(&_lock)).await?;

    let mut conn = db.acquire().await?;
    let row = attachments::get_draft_by_session(&mut conn, &draft.account_id, &draft.session_id)
        .await?
        .ok_or_else(|| AppError::internal("草稿保存后丢失"))?;
    let atts = attachments::list_draft_attachments(&mut conn, &draft.account_id, &row.draft_id).await?;
    draft_from_row(row, atts)
}

/// 清除会话草稿并回收无引用附件。
pub async fn clear_draft(
    db: &SqlitePool,
    paths: &AppPaths,
    account_id: &AccountId,
    session_id: &SessionId,
) -> Result<(), AppError> {
    let _lock = attachment_store::lock_account(&paths.attachments_dir, account_id).map_err(
        |err| match err {
            crate::infrastructure::error::InfraError::Conflict { message } => AppError {
                code: crate::application::error::AppErrorCode::Conflict,
                message,
                retryable: true,
            },
            other => other.into(),
        },
    )?;
    let mut tx = db.begin().await?;
    attachments::delete_draft(&mut tx, account_id, session_id).await?;
    accounts::bump_change_version(&mut tx, account_id).await?;
    tx.commit().await?;
    attachment_use_case::recycle_unreferenced(db, paths, account_id, Some(&_lock)).await?;
    Ok(())
}

/// 清除某账号下全部草稿与附件内容（清理本地数据时用）。
pub async fn clear_drafts_for_account(
    db: &SqlitePool,
    paths: &AppPaths,
    account_id: &AccountId,
) -> Result<(), AppError> {
    let _lock = match attachment_store::lock_account(&paths.attachments_dir, account_id) {
        Ok(lock) => Some(lock),
        Err(crate::infrastructure::error::InfraError::Conflict { .. }) => None,
        Err(err) => return Err(err.into()),
    };
    let mut tx = db.begin().await?;
    attachments::delete_drafts_for_account(&mut tx, account_id).await?;
    // message_attachments 随 messages 删除；此处清内容行
    attachments::delete_contents_for_account(&mut tx, account_id).await?;
    accounts::bump_change_version(&mut tx, account_id).await?;
    tx.commit().await?;
    attachment_use_case::purge_account_files(&paths.attachments_dir, account_id)?;
    Ok(())
}

/// 导入附件（Port 入口）。
pub async fn import_attachment(
    db: &SqlitePool,
    paths: &AppPaths,
    cmd: ImportAttachmentCommand,
) -> Result<ComposeAttachmentDraft, AppError> {
    attachment_use_case::import_attachment(db, paths, cmd).await
}


/// 持久化不可变外发快照：MIME 字节进受管理内容存储，元数据进 outbound_snapshots。
///
/// 调用方必须已持有该账号附件锁（读取附件与安装 MIME 在同一锁窗口内）。
async fn persist_snapshot(
    tx: &mut sqlx::SqliteConnection,
    paths: &AppPaths,
    input: &SnapshotInput,
) -> Result<(String, String), AppError> {
    let internet_message_id = new_internet_message_id();
    let built = build_snapshot(input, &internet_message_id).map_err(AppError::from)?;

    // MIME 字节复用受管理内容存储（同名同摘要去重由调用方保证一次性）
    let (temp_path, digest) = attachment_store::write_temp(
        &paths.attachments_dir,
        &input.account_id,
        &built.mime,
    )
    .map_err(AppError::from)?;
    let mime_content_id = format!("mime-{}", Uuid::new_v4().as_hyphenated());
    let installed = attachment_store::install_from_temp(
        &paths.attachments_dir,
        &input.account_id,
        &temp_path,
        &mime_content_id,
        built.mime.len() as u64,
        &digest,
    )
    .map_err(AppError::from)?;
    let storage_key = installed
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&mime_content_id)
        .to_string();

    attachments::insert_content(
        tx,
        &attachments::AttachmentContentRow {
            content_id: mime_content_id.clone(),
            account_id: input.account_id.clone(),
            storage_key,
            byte_length: built.mime.len() as i64,
            digest,
            created_at: now_ms(),
        },
    )
    .await
    .map_err(AppError::from)?;

    let snapshot_id = format!("snap-{}", Uuid::new_v4().as_hyphenated());
    let envelope_json = serde_json::to_string(&built.envelope)
        .map_err(|e| AppError::internal(format!("envelope 序列化失败: {e}")))?;
    let row = outbound_snapshots::OutboundSnapshotRow {
        snapshot_id: snapshot_id.clone(),
        account_id: input.account_id.clone(),
        content_version: 1,
        internet_message_id: built.internet_message_id.clone(),
        date: built.date.clone(),
        mime_content_id,
        envelope_json,
        subject: built.subject.clone(),
        text_body: built.text_body.clone(),
        html_body: built.html_body.clone(),
        in_reply_to: built.in_reply_to.clone(),
        references: built.references.clone(),
        created_at: now_ms(),
    };
    let atts: Vec<outbound_snapshots::SnapshotAttachmentRow> = built
        .attachments
        .iter()
        .map(|a| outbound_snapshots::SnapshotAttachmentRow {
            content_id: a.content_id.clone(),
            name: a.name.clone(),
            mime: a.mime.clone(),
            sort_order: a.sort_order,
        })
        .collect();
    outbound_snapshots::insert_snapshot(tx, &row, &atts)
        .await
        .map_err(AppError::from)?;
    Ok((snapshot_id, built.internet_message_id))
}

fn snapshot_recipients(list: &[ComposeRecipientRef]) -> Vec<SnapshotRecipient> {
    list.iter()
        .map(|r| SnapshotRecipient {
            address: r.address.as_str().to_string(),
            name: r.display_name.clone(),
        })
        .collect()
}

/// 本地发送：落库 outbound 邮件到指定 Session；清除草稿。附件发送仍拒绝至 Task 08。
pub async fn send(
    db: &SqlitePool,
    paths: &AppPaths,
    cmd: SendComposedMailCommand,
) -> Result<MessageWithRecipients, AppError> {
    if !cmd.attachments.is_empty() {
        return Err(AppError::validation(
            "附件内容存储尚未接通发送通道，无法发送含附件的邮件；草稿已保留",
        ));
    }

    // 持锁覆盖附件读取与 MIME 持久化；失败即回滚，草稿保持原样
    let _lock = attachment_store::lock_account(&paths.attachments_dir, &cmd.account_id).map_err(
        |err| match err {
            crate::infrastructure::error::InfraError::Conflict { message } => AppError {
                code: AppErrorCode::Conflict,
                message,
                retryable: true,
            },
            other => other.into(),
        },
    )?;

    let mut tx = db.begin().await?;

    let account = accounts::get(&mut tx, &cmd.account_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("账户不存在: {}", cmd.account_id)))?;

    let session = sessions::get(&mut tx, &cmd.account_id, &cmd.session_id)
        .await?
        .ok_or_else(|| AppError::not_found("会话不存在"))?;
    let session_id = if let Some(merged) = &session.merged_into_id {
        sessions::resolve_root(&mut tx, &cmd.account_id, merged)
            .await?
            .map(|s| s.id)
            .unwrap_or(session.id)
    } else {
        session.id
    };

    let to = normalize_recipients(&cmd.to);
    let cc = normalize_recipients(&cmd.cc);
    let bcc = normalize_recipients(&cmd.bcc);
    if to.is_empty() && cc.is_empty() && bcc.is_empty() {
        return Err(AppError::validation("请至少填写一位收件人"));
    }

    let plain = strip_html(&cmd.body_html);
    if plain.is_empty() && cmd.attachments.is_empty() {
        return Err(AppError::validation("正文或附件不能为空"));
    }

    let mut in_reply_to: Option<String> = None;
    let mut references: Option<String> = None;
    let mut subject = cmd.subject.trim().to_string();

    if let Some(reply_id) = &cmd.reply_to_message_id {
        let reply = messages::get(&mut tx, &cmd.account_id, reply_id)
            .await?
            .ok_or_else(|| AppError::not_found("回复目标邮件不存在"))?;
        if reply.message.sync_status == MessageSyncStatus::DeletedRemote {
            return Err(AppError::not_found("回复目标邮件不存在"));
        }
        in_reply_to = reply.message.internet_message_id.clone();
        // 父信完整 References 链在后续 MIME 摄取任务补齐；当前至少包含父信 ID
        references = build_references(None, reply.message.internet_message_id.as_deref());
        if subject.is_empty() {
            subject = reply_subject(&reply.message.subject);
        }
    }

    // 首次派发前固定 Message-ID / Date / MIME / envelope
    let mut attachment_bytes = Vec::new();
    for draft in &cmd.attachments {
        let row = attachments::get_content(&mut tx, &cmd.account_id, &draft.content_id)
            .await?
            .ok_or_else(|| AppError::not_found(format!("附件内容不存在: {}", draft.content_id)))?;
        let bytes = attachment_store::load_bytes(
            &paths.attachments_dir,
            &cmd.account_id,
            &row.storage_key,
        )
        .map_err(AppError::from)?;
        attachment_bytes.push(SnapshotAttachmentBytes {
            content_id: draft.content_id.clone(),
            name: draft.name.clone(),
            mime: draft.mime.clone(),
            sort_order: attachment_bytes.len() as i32,
            bytes,
        });
    }
    let snapshot_input = SnapshotInput {
        account_id: cmd.account_id.clone(),
        from: account.email.as_str().to_string(),
        from_name: if account.display_name.trim().is_empty() {
            account.email.as_str().to_string()
        } else {
            account.display_name.clone()
        },
        to: snapshot_recipients(&to),
        cc: snapshot_recipients(&cc),
        bcc: snapshot_recipients(&bcc),
        subject: subject.clone(),
        text_body: plain.clone(),
        html_body: Some(cmd.body_html.clone()).filter(|h| !h.trim().is_empty()),
        in_reply_to: in_reply_to.clone(),
        references: references.clone(),
        attachments: attachment_bytes,
    };
    let (_snapshot_id, internet_message_id) = persist_snapshot(&mut tx, paths, &snapshot_input).await?;

    let t = now_ms();
    let message_id = MessageId::new(next_local_id("msg"));
    let message = Message {
        id: message_id.clone(),
        account_id: cmd.account_id.clone(),
        session_id: session_id.clone(),
        remote_key: None,
        location_status: None,
        location_revision: 0,
        internet_message_id: Some(internet_message_id),
        in_reply_to,
        subject,
        body_preview: plain.chars().take(160).collect(),
        body_content_type: BodyContentType::Html,
        body_content: Some(cmd.body_html.clone()),
        from_address: Some(account.email.clone()),
        from_name: if account.display_name.trim().is_empty() {
            account.email.as_str().to_string()
        } else {
            account.display_name.clone()
        },
        received_at: None,
        sent_at: Some(t),
        is_read: true,
        is_draft: false,
        remote_is_read: None,
        remote_is_draft: None,
        remote_is_deleted: None,
        is_outbound: true,
        has_attachments: !cmd.attachments.is_empty(),
        importance: Importance::Normal,
        folder_id: None,
        parent_folder_name: Some("已发送".into()),
        sync_status: MessageSyncStatus::PendingLocal,
        local_updated_at: t,
        remote_updated_at: None,
    };

    let mut recipients = Vec::new();
    let mut push_role = |role: RecipientRole, list: &[ComposeRecipientRef]| {
        for (seq, r) in list.iter().enumerate() {
            recipients.push(MessageRecipient {
                account_id: cmd.account_id.clone(),
                message_id: message_id.clone(),
                role,
                seq: seq as i32,
                normalized_address: r.address.clone(),
                display_name: r.display_name.clone(),
            });
        }
    };
    push_role(RecipientRole::To, &to);
    push_role(RecipientRole::Cc, &cc);
    push_role(RecipientRole::Bcc, &bcc);

    let bundle = MessageWithRecipients {
        message,
        recipients,
    };
    messages::upsert_with_recipients(&mut tx, &bundle).await?;
    sessions::recompute_projection(&mut tx, &cmd.account_id, &session_id).await?;
    crate::infrastructure::repository::jobs::enqueue_or_reset(
        &mut tx,
        &cmd.account_id,
        &message_id,
        t,
    )
    .await?;
    attachments::delete_draft(&mut tx, &cmd.account_id, &cmd.session_id).await?;
    accounts::bump_change_version(&mut tx, &cmd.account_id).await?;
    tx.commit().await?;

    if let Ok(lock) = attachment_store::lock_account(&paths.attachments_dir, &cmd.account_id) {
        let _ = attachment_use_case::recycle_unreferenced(
            db,
            paths,
            &cmd.account_id,
            Some(&lock),
        )
        .await;
    }

    let _ = crate::application::use_cases::outbound_use_case::flush_pending_message(
        db,
        paths,
        &cmd.account_id,
        &message_id,
    )
    .await;

    Ok(bundle)
}
