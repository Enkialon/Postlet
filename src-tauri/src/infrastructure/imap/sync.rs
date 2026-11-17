//! IMAP 文件夹 UID 增量同步、FLAGS 与删除检测（阻塞 API）
//!
//! 按服务器 UID 集合与本地已下载集合的差集分批补齐；允许 UID 空洞，失败项留待下轮。

use super::client::ImapSession;
use super::parse::{decode_remote_key, parse_rfc822, remote_id};
use crate::domain::mail::{RemoteMessageChange, RemoteMessage};
use crate::application::error::PortError;
use crate::domain::services::{imap_pending_boundary, missing_imap_page};
use std::collections::{HashMap, HashSet};

/// QQ 等对大包不友好，控制批量大小
const FETCH_CHUNK: usize = 8;

#[derive(Debug, Clone)]
pub struct FolderSyncResult {
    pub folder: String,
    pub uidvalidity: u32,
    pub generation_changed: bool,
    /// 向前水位
    pub last_uid: u32,
    /// 最新未下载 UID 的上界；1 表示当前快照已全部下载。
    pub low_uid: u32,
    pub changes: Vec<RemoteMessageChange>,
    pub missing_remote_ids: Vec<String>,
}

/// 同步单个邮箱（阻塞）。
pub fn sync_folder(
    session: &mut ImapSession,
    folder: &str,
    stored_generation: Option<&str>,
    known_messages: &[(crate::domain::value_objects::RemoteKey, bool)],
    page_limit: u32,
) -> Result<FolderSyncResult, PortError> {
    let mailbox = session.select(folder).map_err(|e| PortError::Remote {
        message: format!("IMAP SELECT {folder} 失败: {e}"),
        retryable: true,
    })?;
    let uidvalidity = mailbox.uid_validity.filter(|v| *v > 0).ok_or_else(|| PortError::Remote {
        message: "IMAP SELECT 未返回有效 UIDVALIDITY，已停止同步".into(), retryable: false,
    })?;
    let known_uids: Vec<(u32, bool)> = known_messages.iter().filter_map(|(key, seen)| {
        let (generation, uid) = decode_remote_key(key).ok()?;
        (generation == uidvalidity).then_some((uid, *seen))
    }).collect();
    let current_generation = uidvalidity.to_string();
    let same_generation = stored_generation == Some(current_generation.as_str());
    // 游标只记录进度，不能代替远端实际 UID 集合：旧水位可能已经跳过缺口。
    let known: HashSet<u32> = if same_generation {
        known_uids.iter().map(|(uid, _)| *uid).collect()
    } else {
        HashSet::new()
    };
    let remote_uids = search_uids(session, "ALL")?;
    let page = missing_imap_page(&remote_uids, &known, page_limit);
    let fetched = fetch_uid_page(session, folder, uidvalidity, &page)?;
    if !page.is_empty() && fetched.ok_uids.is_empty() {
        return Err(PortError::Remote {
            message: format!(
                "IMAP 文件夹 {folder} 请求 {} 封，但无法读取正文，稍后重试",
                page.len()
            ),
            retryable: true,
        });
    }
    let mut downloaded = known;
    downloaded.extend(fetched.ok_uids);
    let high_uid = downloaded
        .intersection(&remote_uids)
        .copied()
        .max()
        .unwrap_or(0);
    let low_uid = imap_pending_boundary(&remote_uids, &downloaded);
    eprintln!(
        "IMAP 同步 {folder}: 远端={}，本批下载={}，待下载={}",
        remote_uids.len(),
        page.iter().filter(|uid| downloaded.contains(uid)).count(),
        remote_uids.difference(&downloaded).count(),
    );
    let mut changes = fetched.changes;
    // 删除 / FLAGS
    let mut missing_remote_ids = Vec::new();
    if same_generation && !known_uids.is_empty() {
        let previous_flags: HashMap<_, _> = known_uids.iter().copied().collect();
        let mut present = Vec::new();
        for (uid, _) in &known_uids {
            if !remote_uids.contains(uid) {
                missing_remote_ids.push(remote_id(uidvalidity, *uid).as_str().to_string());
                continue;
            }
            present.push(*uid);
        }
        // 历史越多也不能逐封往返，否则回填最终会被 FLAGS 核对耗尽总时限。
        for chunk in present.chunks(200) {
            let set = chunk
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let fetches = session
                .uid_fetch(set, "(UID FLAGS)")
                .map_err(|e| PortError::Remote {
                    message: format!("IMAP FLAGS FETCH 失败: {e}"),
                    retryable: true,
                })?;
            for item in fetches.iter() {
                let Some(uid) = item.uid else { continue };
                let Some(was_read) = previous_flags.get(&uid) else {
                    continue;
                };
                let is_read = item
                    .flags()
                    .iter()
                    .any(|f| matches!(f, imap::types::Flag::Seen));
                if is_read != *was_read {
                    changes.push(RemoteMessageChange::Upsert(flag_only_remote(
                        folder, uidvalidity, uid, is_read,
                    )));
                }
            }
        }
    }

    Ok(FolderSyncResult {
        folder: folder.to_string(),
        uidvalidity,
        generation_changed: stored_generation.is_some_and(|generation| generation != current_generation),
        last_uid: high_uid,
        low_uid,
        changes,
        missing_remote_ids,
    })
}

fn search_uids(session: &mut ImapSession, query: &str) -> Result<HashSet<u32>, PortError> {
    session.uid_search(query).map_err(|e| PortError::Remote {
        message: format!("IMAP UID SEARCH 失败 ({query}): {e}"),
        retryable: true,
    })
}

struct FetchPageResult {
    changes: Vec<RemoteMessageChange>,
    ok_uids: HashSet<u32>,
}

fn fetch_uid_page(
    session: &mut ImapSession,
    folder: &str,
    uidvalidity: u32,
    uid_list: &[u32],
) -> Result<FetchPageResult, PortError> {
    let mut changes = Vec::new();
    let mut ok_uids = HashSet::new();
    if uid_list.is_empty() {
        return Ok(FetchPageResult { changes, ok_uids });
    }

    for chunk in uid_list.chunks(FETCH_CHUNK) {
        let set = chunk
            .iter()
            .map(|u| u.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let fetches = session
            .uid_fetch(&set, "(FLAGS INTERNALDATE BODY.PEEK[])")
            .or_else(|_| session.uid_fetch(&set, "(FLAGS BODY.PEEK[])"))
            .map_err(|e| PortError::Remote {
                message: format!("IMAP UID FETCH 失败: {e}"),
                retryable: true,
            })?;

        for item in fetches.iter() {
            let Some(uid) = item.uid else {
                continue;
            };
            let is_read = item
                .flags()
                .iter()
                .any(|f| matches!(f, imap::types::Flag::Seen));
            let Some(body) = item.body() else {
                continue;
            };
            let internal_ms = item.internal_date().map(|dt| dt.timestamp_millis());
            match parse_rfc822(folder, uidvalidity, uid, body, is_read, internal_ms) {
                Ok(remote) => {
                    ok_uids.insert(uid);
                    changes.push(RemoteMessageChange::Upsert(remote));
                }
                Err(_) => {
                    // 单封解析失败不中断整夹，下轮可重试
                }
            }
        }

        for uid in chunk {
            if ok_uids.contains(uid) {
                continue;
            }
            // 批量未给出正文或解析失败 → 单封再试
            match fetch_one_rfc822(session, folder, uidvalidity, *uid)? {
                Some(remote) => {
                    ok_uids.insert(*uid);
                    changes.push(RemoteMessageChange::Upsert(remote));
                }
                None => {}
            }
        }
    }

    Ok(FetchPageResult { changes, ok_uids })
}

fn fetch_one_rfc822(
    session: &mut ImapSession,
    folder: &str,
    uidvalidity: u32,
    uid: u32,
) -> Result<Option<RemoteMessage>, PortError> {
    let fetches = session
        .uid_fetch(uid.to_string(), "(FLAGS INTERNALDATE BODY.PEEK[])")
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
        match parse_rfc822(folder, uidvalidity, uid, body, is_read, internal_ms) {
            Ok(remote) => return Ok(Some(remote)),
            Err(_) => return Ok(None),
        }
    }
    Ok(None)
}

fn flag_only_remote(folder: &str, uidvalidity: u32, uid: u32, is_read: bool) -> RemoteMessage {
    RemoteMessage {
        remote_key: remote_id(uidvalidity, uid),
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
        is_read,
        is_draft: false,
        has_attachments: false,
        importance: crate::domain::value_objects::Importance::Normal,
        folder_id: Some(crate::domain::value_objects::FolderId::new(
            folder.to_string(),
        )),
        parent_folder_name: Some(folder.to_string()),
        remote_updated_at: None,
    }
}
