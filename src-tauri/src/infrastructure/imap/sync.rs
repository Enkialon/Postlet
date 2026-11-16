//! IMAP 文件夹 UID 增量同步、FLAGS 与删除检测（阻塞 API）

use super::client::ImapSession;
use super::parse::{parse_rfc822, remote_id};
use crate::application::dto::remote_models::{GraphMessageChange, RemoteMessage};
use crate::application::error::PortError;
use crate::infrastructure::mail::cursor::parse_cursor;

#[derive(Debug, Clone)]
pub struct FolderSyncResult {
    pub folder: String,
    pub uidvalidity: u32,
    pub last_uid: u32,
    pub changes: Vec<GraphMessageChange>,
    pub missing_remote_ids: Vec<String>,
}

/// 同步单个邮箱（阻塞）。
pub fn sync_folder(
    session: &mut ImapSession,
    folder: &str,
    stored_delta: Option<&str>,
    stored_next: Option<&str>,
    known_uids: &[(u32, bool)],
    page_limit: u32,
) -> Result<FolderSyncResult, PortError> {
    let mailbox = session.select(folder).map_err(|e| PortError::Remote {
        message: format!("IMAP SELECT {folder} 失败: {e}"),
        retryable: true,
    })?;
    let uidvalidity = mailbox.uid_validity.unwrap_or(0);
    let stored = parse_cursor(stored_delta, stored_next);
    let mut last_uid = 0_u32;
    let mut full_resync = stored.is_none();
    if let Some(c) = stored {
        if c.uidvalidity != uidvalidity {
            full_resync = true;
        } else {
            last_uid = c.last_uid;
        }
    }

    // UID SEARCH 的参数是 SEARCH criteria，不是 sequence-set。
    // 增量必须用 `UID n:*`；`n:*` 会被当成序号，常见邮箱 UID 很大时会搜到空集。
    let search_query = if full_resync || last_uid == 0 {
        "ALL".to_string()
    } else {
        format!("UID {}:{}", last_uid + 1, "*")
    };

    let uids = session
        .uid_search(&search_query)
        .map_err(|e| PortError::Remote {
            message: format!("IMAP UID SEARCH 失败 ({search_query}): {e}"),
            retryable: true,
        })?;
    let mut uid_list: Vec<u32> = uids.into_iter().collect();
    uid_list.sort_unstable();
    // 会话优先：分页时留最新 UID，而不是截掉远端大量新信只留最旧一批。
    if page_limit > 0 && uid_list.len() > page_limit as usize {
        let keep_from = uid_list.len() - page_limit as usize;
        uid_list = uid_list.split_off(keep_from);
    }

    let mut changes = Vec::new();
    let mut max_uid = last_uid;
    let mut fetched_ok = 0_u32;
    let mut missing_body = 0_u32;

    // 批量 FETCH，减少往返；单 UID 失败时再降级。
    for chunk in uid_list.chunks(25) {
        let set = chunk
            .iter()
            .map(|u| u.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let fetches = session
            .uid_fetch(&set, "(FLAGS INTERNALDATE RFC822)")
            .or_else(|_| session.uid_fetch(&set, "(FLAGS INTERNALDATE BODY.PEEK[])"))
            .or_else(|_| session.uid_fetch(&set, "(FLAGS RFC822)"))
            .or_else(|_| session.uid_fetch(&set, "(FLAGS BODY.PEEK[])"))
            .map_err(|e| PortError::Remote {
                message: format!("IMAP UID FETCH 失败: {e}"),
                retryable: true,
            })?;

        let mut seen_uids = std::collections::HashSet::<u32>::new();
        for item in fetches.iter() {
            let Some(uid) = item.uid else {
                continue;
            };
            if !full_resync && uid <= last_uid {
                continue;
            }
            seen_uids.insert(uid);
            let is_read = item
                .flags()
                .iter()
                .any(|f| matches!(f, imap::types::Flag::Seen));
            let Some(body) = item.body() else {
                missing_body += 1;
                continue;
            };
            let internal_ms = item.internal_date().map(|dt| dt.timestamp_millis());
            let remote = parse_rfc822(folder, uid, body, is_read, internal_ms).map_err(|e| {
                PortError::Remote {
                    message: format!("解析邮件 UID={uid} 失败: {e}"),
                    retryable: false,
                }
            })?;
            max_uid = max_uid.max(uid);
            fetched_ok += 1;
            changes.push(GraphMessageChange::Upsert(remote));
        }

        // 批量响应里缺正文的 UID，单封用 RFC822 再试一次。
        for uid in chunk {
            if seen_uids.contains(uid) {
                continue;
            }
            if !full_resync && *uid <= last_uid {
                continue;
            }
            match fetch_one_rfc822(session, folder, *uid)? {
                Some(remote) => {
                    max_uid = max_uid.max(*uid);
                    fetched_ok += 1;
                    changes.push(GraphMessageChange::Upsert(remote));
                }
                None => missing_body += 1,
            }
        }
    }

    if !uid_list.is_empty() && fetched_ok == 0 {
        return Err(PortError::Remote {
            message: format!(
                "IMAP 文件夹 {folder} 搜到 {} 封，但无法读取正文（RFC822/BODY 均空，missing={missing_body}）",
                uid_list.len()
            ),
            retryable: true,
        });
    }

    let mut missing_remote_ids = Vec::new();
    if !known_uids.is_empty() {
        let remote_uids = session.uid_search("ALL").map_err(|e| PortError::Remote {
            message: format!("IMAP UID SEARCH 全量失败: {e}"),
            retryable: true,
        })?;
        for (uid, was_read) in known_uids {
            if !remote_uids.contains(uid) {
                missing_remote_ids.push(remote_id(folder, *uid).as_str().to_string());
                continue;
            }
            let fetches = session
                .uid_fetch(uid.to_string(), "(FLAGS)")
                .map_err(|e| PortError::Remote {
                    message: format!("IMAP FLAGS FETCH 失败: {e}"),
                    retryable: true,
                })?;
            for item in fetches.iter() {
                let is_read = item
                    .flags()
                    .iter()
                    .any(|f| matches!(f, imap::types::Flag::Seen));
                if is_read != *was_read {
                    changes.push(GraphMessageChange::Upsert(flag_only_remote(
                        folder, *uid, is_read,
                    )));
                }
            }
        }
        // 不在删除扫描里抬高 last_uid，避免跳过尚未 FETCH 的新信。
    }

    Ok(FolderSyncResult {
        folder: folder.to_string(),
        uidvalidity,
        last_uid: max_uid,
        changes,
        missing_remote_ids,
    })
}

fn fetch_one_rfc822(
    session: &mut ImapSession,
    folder: &str,
    uid: u32,
) -> Result<Option<RemoteMessage>, PortError> {
    let fetches = session
        .uid_fetch(uid.to_string(), "(FLAGS INTERNALDATE RFC822)")
        .or_else(|_| session.uid_fetch(uid.to_string(), "(FLAGS INTERNALDATE BODY.PEEK[])"))
        .or_else(|_| session.uid_fetch(uid.to_string(), "(FLAGS RFC822)"))
        .or_else(|_| session.uid_fetch(uid.to_string(), "(FLAGS BODY.PEEK[])"))
        .map_err(|e| PortError::Remote {
            message: format!("IMAP UID FETCH 单封失败 UID={uid}: {e}"),
            retryable: true,
        })?;
    for item in fetches.iter() {
        let is_read = item
            .flags()
            .iter()
            .any(|f| matches!(f, imap::types::Flag::Seen));
        let Some(body) = item.body() else {
            continue;
        };
        let internal_ms = item.internal_date().map(|dt| dt.timestamp_millis());
        return Ok(Some(parse_rfc822(folder, uid, body, is_read, internal_ms).map_err(
            |e| PortError::Remote {
                message: format!("解析邮件 UID={uid} 失败: {e}"),
                retryable: false,
            },
        )?));
    }
    Ok(None)
}

fn flag_only_remote(folder: &str, uid: u32, is_read: bool) -> RemoteMessage {
    RemoteMessage {
        graph_id: remote_id(folder, uid),
        graph_change_key: Some(format!("uid:{uid}")),
        internet_message_id: None,
        in_reply_to: None,
        subject: String::new(),
        body_preview: String::new(),
        body_content_type: crate::domain::value_objects::BodyContentType::Text,
        body_content: None,
        from_address: None,
        from_name: String::new(),
        recipients: Vec::new(),
        received_at: None,
        sent_at: None,
        created_at_graph: None,
        is_read,
        is_draft: false,
        has_attachments: false,
        importance: crate::domain::value_objects::Importance::Normal,
        folder_id: Some(crate::domain::value_objects::FolderId::new(folder.to_string())),
        parent_folder_name: Some(folder.to_string()),
        remote_updated_at: None,
    }
}
