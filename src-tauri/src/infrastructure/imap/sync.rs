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

    let search_set = if full_resync || last_uid == 0 {
        "1:*".to_string()
    } else {
        format!("{}:*", last_uid + 1)
    };

    let uids = session.uid_search(search_set).map_err(|e| PortError::Remote {
        message: format!("IMAP UID SEARCH 失败: {e}"),
        retryable: true,
    })?;
    let mut uid_list: Vec<u32> = uids.into_iter().collect();
    uid_list.sort_unstable();
    if page_limit > 0 && uid_list.len() > page_limit as usize {
        uid_list.truncate(page_limit as usize);
    }

    let mut changes = Vec::new();
    let mut max_uid = last_uid;
    for uid in uid_list {
        if !full_resync && uid <= last_uid {
            continue;
        }
        let fetches = session
            .uid_fetch(uid.to_string(), "(FLAGS BODY.PEEK[])")
            .map_err(|e| PortError::Remote {
                message: format!("IMAP UID FETCH 失败: {e}"),
                retryable: true,
            })?;
        for item in fetches.iter() {
            let is_read = item.flags().iter().any(|f| matches!(f, imap::types::Flag::Seen));
            let Some(body) = item.body() else {
                continue;
            };
            let remote = parse_rfc822(folder, uid, body, is_read).map_err(|e| PortError::Remote {
                message: format!("解析邮件 UID={uid} 失败: {e}"),
                retryable: false,
            })?;
            max_uid = max_uid.max(uid);
            changes.push(GraphMessageChange::Upsert(remote));
        }
    }

    let mut missing_remote_ids = Vec::new();
    if !known_uids.is_empty() {
        let remote_uids = session.uid_search("1:*").map_err(|e| PortError::Remote {
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
                let is_read = item.flags().iter().any(|f| matches!(f, imap::types::Flag::Seen));
                if is_read != *was_read {
                    changes.push(GraphMessageChange::Upsert(flag_only_remote(
                        folder, *uid, is_read,
                    )));
                }
            }
        }
        for uid in &remote_uids {
            max_uid = max_uid.max(*uid);
        }
    }

    Ok(FolderSyncResult {
        folder: folder.to_string(),
        uidvalidity,
        last_uid: max_uid,
        changes,
        missing_remote_ids,
    })
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
