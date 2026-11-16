//! 本地写信用例：进程内草稿 + SQLite 发送落库（不接 Graph）

use crate::application::dto::{
    ComposeDraft, ComposeRecipientRef, SaveComposeDraftCommand, SendComposedMailCommand,
};
use crate::application::error::AppError;
use crate::domain::entities::{Message, MessageRecipient, MessageWithRecipients};
use crate::domain::value_objects::{
    AccountId, BodyContentType, EmailAddress, Importance, MessageId, MessageSyncStatus,
    RecipientRole, SessionId, TimestampMs,
};
use crate::infrastructure::repository::{accounts, messages, sessions};
use crate::AppState;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

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

fn draft_key(account_id: &AccountId, session_id: &SessionId) -> String {
    format!("{}:{}", account_id.as_str(), session_id.as_str())
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

/// 读取会话草稿；无则 `None`
pub fn get_draft(
    state: &AppState,
    account_id: &AccountId,
    session_id: &SessionId,
) -> Result<Option<ComposeDraft>, AppError> {
    let key = draft_key(account_id, session_id);
    let guard = state
        .compose_drafts
        .lock()
        .map_err(|_| AppError::internal("草稿锁损坏"))?;
    Ok(guard.get(&key).cloned())
}

/// 保存会话草稿（进程内）
pub async fn save_draft(
    state: &AppState,
    cmd: SaveComposeDraftCommand,
) -> Result<ComposeDraft, AppError> {
    {
        let mut conn = state.db.acquire().await?;
        accounts::get(&mut conn, &cmd.account_id)
            .await?
            .ok_or_else(|| AppError::not_found(format!("账户不存在: {}", cmd.account_id)))?;
    }

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
        updated_at: now_ms(),
    };
    let key = draft_key(&draft.account_id, &draft.session_id);
    let mut guard = state
        .compose_drafts
        .lock()
        .map_err(|_| AppError::internal("草稿锁损坏"))?;
    guard.insert(key, draft.clone());
    Ok(draft)
}

/// 清除会话草稿
pub fn clear_draft(
    state: &AppState,
    account_id: &AccountId,
    session_id: &SessionId,
) -> Result<(), AppError> {
    let key = draft_key(account_id, session_id);
    let mut guard = state
        .compose_drafts
        .lock()
        .map_err(|_| AppError::internal("草稿锁损坏"))?;
    guard.remove(&key);
    Ok(())
}

/// 本地发送：落库 outbound 邮件到指定 Session；清除草稿
pub async fn send(
    state: &AppState,
    cmd: SendComposedMailCommand,
) -> Result<MessageWithRecipients, AppError> {
    let db: &SqlitePool = &state.db;
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
    let mut subject = cmd.subject.trim().to_string();

    if let Some(reply_id) = &cmd.reply_to_message_id {
        let reply = messages::get(&mut tx, &cmd.account_id, reply_id)
            .await?
            .ok_or_else(|| AppError::not_found("回复目标邮件不存在"))?;
        if reply.message.sync_status == MessageSyncStatus::DeletedRemote {
            return Err(AppError::not_found("回复目标邮件不存在"));
        }
        in_reply_to = reply.message.internet_message_id.clone();
        if subject.is_empty() {
            subject = reply_subject(&reply.message.subject);
        }
    }

    let t = now_ms();
    let message_id = MessageId::new(next_local_id("msg"));
    let message = Message {
        id: message_id.clone(),
        account_id: cmd.account_id.clone(),
        session_id: session_id.clone(),
        graph_id: None,
        graph_change_key: None,
        internet_message_id: None,
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
        created_at_graph: None,
        is_read: true,
        is_draft: false,
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
    tx.commit().await?;

    let key = draft_key(&cmd.account_id, &cmd.session_id);
    if let Ok(mut guard) = state.compose_drafts.lock() {
        guard.remove(&key);
    }

    let _ = crate::application::use_cases::outbound_use_case::flush_pending_message(
        &state.db,
        &state.paths,
        &cmd.account_id,
        &message_id,
    )
    .await;

    Ok(bundle)
}

/// 空草稿表（供 AppState 初始化）
pub fn empty_draft_map() -> HashMap<String, ComposeDraft> {
    HashMap::new()
}
