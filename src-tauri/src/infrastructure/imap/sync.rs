//! IMAP 文件夹 UID 增量同步、FLAGS 与删除检测（阻塞 API）
//!
//! 按服务器 UID 集合与本地已下载集合的差集分批补齐；允许 UID 空洞，失败项留待下轮。

use super::client::ImapSession;
use crate::infrastructure::mail::operation::MailOperation;
use super::parse::{decode_remote_key, parse_rfc822, remote_id};
use crate::domain::mail::{RemoteFlags, RemoteMessageChange, RemoteMessage};
use crate::infrastructure::error::InfraError;
use crate::domain::services::{imap_pending_boundary, missing_imap_page};
use std::collections::{HashMap, HashSet};

/// QQ 等对大包不友好，控制批量大小
const FETCH_CHUNK: usize = 8;

#[derive(Debug, Clone)]
pub struct FolderSyncRequest {
    pub folder_id: crate::domain::value_objects::FolderId,
    pub remote_binding: String,
    pub binding_revision: i64,
    pub stored_generation: Option<String>,
    pub known_messages: Vec<(crate::domain::value_objects::RemoteKey, bool)>,
    /// 仍在失败退避中的候选位置；本轮不得占用下载页。
    pub cooling_candidates: Vec<crate::domain::value_objects::RemoteKey>,
}

#[derive(Debug, Clone)]
pub struct FolderSyncResult {
    pub folder_id: crate::domain::value_objects::FolderId,
    pub binding_revision: i64,
    pub uidvalidity: u32,
    pub generation_changed: bool,
    /// 向前水位
    pub last_uid: u32,
    /// 最新未下载 UID 的上界；1 表示当前快照已全部下载。
    pub low_uid: u32,
    pub changes: Vec<RemoteMessageChange>,
    pub missing_remote_ids: Vec<String>,
    /// 本批解析/内容失败、需退避重试的 UID（不计为已下载）
    pub failed_uids: Vec<u32>,
    /// 本轮远端发现到但本地尚未下载的位置；由 application 写入候选表。
    pub discovered_remote_ids: Vec<String>,
    /// 该文件夹本轮不可用（无 UIDVALIDITY / SELECT 失败等）；不推进游标，但不影响其他夹
    pub folder_error: Option<String>,
}

/// 同步单个邮箱（阻塞）。
pub fn sync_folder(
    session: &mut ImapSession,
    request: &FolderSyncRequest,
    page_limit: u32,
    operation: &MailOperation,
) -> Result<FolderSyncResult, InfraError> {
    operation.checkpoint()?;
    let folder = request.remote_binding.as_str();
    let stored_generation = request.stored_generation.as_deref();
    let known_messages = &request.known_messages;
    // 夹级错误只影响本夹：不推进游标，交由上层继续其他文件夹
    let mailbox = match session.select(folder) {
        Ok(mailbox) => mailbox,
        Err(e) => {
            return Ok(FolderSyncResult::unavailable(
                request,
                format!("IMAP SELECT {folder} 失败: {e}"),
            ))
        }
    };
    let Some(uidvalidity) = mailbox.uid_validity.filter(|v| *v > 0) else {
        return Ok(FolderSyncResult::unavailable(
            request,
            format!("IMAP SELECT {folder} 未返回有效 UIDVALIDITY，本轮跳过该文件夹"),
        ));
    };
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
    let cooled: HashSet<u32> = request
        .cooling_candidates
        .iter()
        .filter_map(|key| {
            let (generation, uid) = decode_remote_key(key).ok()?;
            (generation == uidvalidity).then_some(uid)
        })
        .collect();
    let eligible: HashSet<u32> = remote_uids.difference(&cooled).copied().collect();
    let page = missing_imap_page(&eligible, &known, page_limit);
    let fetched = fetch_uid_page(session, &request.folder_id, uidvalidity, &page, operation)?;
    if !page.is_empty() && fetched.ok_uids.is_empty() {
        // 整页不可读：保留失败事实交上层记录退避，不阻塞更旧候选
        eprintln!(
            "IMAP 同步 {folder}: 本批 {} 封均无法读取正文，已记录待重试",
            page.len()
        );
    }
    // 只登记本轮实际尝试的页内候选；未选中的缺失项下轮再进入候选表。
    let discovered_remote_ids: Vec<String> = page
        .iter()
        .map(|uid| remote_id(uidvalidity, *uid).as_str().to_string())
        .collect();
    let failed_uids: Vec<u32> = page
        .iter()
        .copied()
        .filter(|uid| !fetched.ok_uids.contains(uid))
        .collect();
    let mut downloaded = known;
    downloaded.extend(fetched.ok_uids);
    let high_uid = downloaded
        .intersection(&remote_uids)
        .copied()
        .max()
        .unwrap_or(0);
    let low_uid = imap_pending_boundary(&remote_uids, &downloaded);
    eprintln!(
        "[{}] IMAP 同步 {folder:?}: 远端={}，本批下载={}，待下载={}（尚未入库）",
        chrono::Local::now().format("%F %T%.3f"),
        remote_uids.len(),
        page.iter().filter(|uid| downloaded.contains(uid)).count(),
        remote_uids.difference(&downloaded).count(),
    );
    let mut changes = fetched.changes;
    // 删除 / FLAGS
    let mut missing_remote_ids = Vec::new();
    if same_generation {
        for (uid, _) in &known_uids {
            if !remote_uids.contains(uid) {
                missing_remote_ids.push(remote_id(uidvalidity, *uid).as_str().to_string());
            }
        }
    }
    let mut present: Vec<_> = downloaded.intersection(&remote_uids).copied().collect();
    present.sort_unstable();
    let mut observed = HashMap::new();
    for chunk in present.chunks(200) {
        operation.checkpoint()?;
        let set = chunk.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
        let raw = session.run_command_and_read_response(format!("UID FETCH {set} (UID FLAGS)"))
            .map_err(|_| InfraError::Remote { message: "IMAP FLAGS FETCH 失败".into(), retryable: true })?;
        for (uid, flags) in parse_flag_observations(&raw)? {
            if chunk.contains(&uid) { observed.insert(remote_id(uidvalidity, uid), flags); }
        }
    }
    for change in &mut changes {
        if let RemoteMessageChange::Upsert(remote) = change {
            if let Some(flags) = observed.get(&remote.remote_key) { remote.flags = *flags; }
        }
    }
    changes.extend(observed.into_iter().map(|(remote_key, flags)| RemoteMessageChange::FlagsChanged { remote_key, flags }));

    Ok(FolderSyncResult {
        folder_id: request.folder_id.clone(),
        binding_revision: request.binding_revision,
        uidvalidity,
        generation_changed: stored_generation.is_some_and(|generation| generation != current_generation),
        last_uid: high_uid,
        low_uid,
        changes,
        missing_remote_ids,
        failed_uids,
        discovered_remote_ids,
        folder_error: None,
    })
}

impl FolderSyncResult {
    /// 文件夹本轮不可用：保留既有游标与下载事实，不产生变更
    pub(super) fn unavailable(request: &FolderSyncRequest, message: String) -> Self {
        Self {
            folder_id: request.folder_id.clone(),
            binding_revision: request.binding_revision,
            uidvalidity: 0,
            generation_changed: false,
            last_uid: 0,
            low_uid: 0,
            changes: Vec::new(),
            missing_remote_ids: Vec::new(),
            failed_uids: Vec::new(),
            discovered_remote_ids: Vec::new(),
            folder_error: Some(message),
        }
    }
}

fn search_uids(session: &mut ImapSession, query: &str) -> Result<HashSet<u32>, InfraError> {
    session.uid_search(query).map_err(|e| InfraError::Remote {
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
    folder: &crate::domain::value_objects::FolderId,
    uidvalidity: u32,
    uid_list: &[u32],
    operation: &MailOperation,
) -> Result<FetchPageResult, InfraError> {
    let mut changes = Vec::new();
    let mut ok_uids = HashSet::new();
    if uid_list.is_empty() {
        return Ok(FetchPageResult { changes, ok_uids });
    }

    for chunk in uid_list.chunks(FETCH_CHUNK) {
        operation.checkpoint()?;
        let set = chunk
            .iter()
            .map(|u| u.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let fetches = session
            .uid_fetch(&set, "(FLAGS INTERNALDATE BODY.PEEK[])")
            .or_else(|_| session.uid_fetch(&set, "(FLAGS BODY.PEEK[])"))
            .map_err(|e| InfraError::Remote {
                message: format!("IMAP UID FETCH 失败: {e}"),
                retryable: true,
            })?;

        for item in fetches.iter() {
            operation.checkpoint()?;
            let Some(uid) = item.uid else {
                continue;
            };
            let Some(body) = item.body() else {
                continue;
            };
            let internal_ms = item.internal_date().map(|dt| dt.timestamp_millis());
            match parse_rfc822(folder, uidvalidity, uid, body, internal_ms) {
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
            operation.checkpoint()?;
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
    folder: &crate::domain::value_objects::FolderId,
    uidvalidity: u32,
    uid: u32,
) -> Result<Option<RemoteMessage>, InfraError> {
    let fetches = session
        .uid_fetch(uid.to_string(), "(FLAGS INTERNALDATE BODY.PEEK[])")
        .or_else(|_| session.uid_fetch(uid.to_string(), "(FLAGS BODY.PEEK[])"))
        .map_err(|e| InfraError::Remote {
            message: format!("IMAP UID FETCH 单封失败 UID={uid}: {e}"),
            retryable: true,
        })?;
    for item in fetches.iter() {
        let Some(body) = item.body() else {
            continue;
        };
        let internal_ms = item.internal_date().map(|dt| dt.timestamp_millis());
        match parse_rfc822(folder, uidvalidity, uid, body, internal_ms) {
            Ok(remote) => return Ok(Some(remote)),
            Err(_) => return Ok(None),
        }
    }
    Ok(None)
}

/// Use the locked crate's protocol parser: Fetch::flags() alone cannot distinguish
/// an absent FLAGS attribute from an explicitly empty FLAGS list.
fn parse_flag_observations(mut raw: &[u8]) -> Result<Vec<(u32, RemoteFlags)>, InfraError> {
    use imap_proto::{AttributeValue, Response};
    let mut observed = Vec::new();
    while !raw.is_empty() {
        let (rest, response) = imap_proto::parse_response(raw).map_err(|_| InfraError::Remote {
            message: "IMAP FLAGS 响应无法解析".into(), retryable: true,
        })?;
        raw = rest;
        let Response::Fetch(_, attributes) = response else { continue };
        let uid = attributes.iter().find_map(|attribute| match attribute { AttributeValue::Uid(uid) => Some(*uid), _ => None });
        let flags = attributes.iter().find_map(|attribute| match attribute { AttributeValue::Flags(flags) => Some(flags), _ => None });
        if let (Some(uid), Some(flags)) = (uid, flags) {
            if uid == 0 { continue; }
            observed.push((uid, RemoteFlags {
                is_read: Some(flags.iter().any(|flag| flag.eq_ignore_ascii_case("\\Seen"))),
                is_draft: Some(flags.iter().any(|flag| flag.eq_ignore_ascii_case("\\Draft"))),
                is_deleted: Some(flags.iter().any(|flag| flag.eq_ignore_ascii_case("\\Deleted"))),
            }));
        }
    }
    Ok(observed)
}

#[cfg(test)]
mod flag_tests {
    use super::*;

    #[test]
    fn missing_flags_are_unchanged_and_explicit_empty_flags_clear_values() {
        let raw = b"* 1 FETCH (UID 10)\r\n* 2 FETCH (UID 20 FLAGS ())\r\n* 3 FETCH (UID 30 FLAGS (\\Seen \\Draft \\Deleted))\r\n* 4 FETCH (FLAGS (\\Seen))\r\n";
        let values = parse_flag_observations(raw).unwrap();
        assert_eq!(values, vec![
            (20, RemoteFlags { is_read: Some(false), is_draft: Some(false), is_deleted: Some(false) }),
            (30, RemoteFlags { is_read: Some(true), is_draft: Some(true), is_deleted: Some(true) }),
        ]);
        assert!(parse_flag_observations(b"not an IMAP response\r\n").is_err());
    }
}

#[cfg(test)]
mod folder_isolation_tests {
    use super::*;

    fn request() -> FolderSyncRequest {
        FolderSyncRequest {
            folder_id: crate::domain::value_objects::FolderId::new("f1"),
            remote_binding: "Broken".into(),
            binding_revision: 3,
            stored_generation: None,
            known_messages: Vec::new(),
            cooling_candidates: Vec::new(),
        }
    }

    #[test]
    fn unavailable_folder_keeps_progress_and_reports_error() {
        let result = FolderSyncResult::unavailable(&request(), "no uidvalidity".into());
        assert_eq!(result.binding_revision, 3);
        assert_eq!(result.uidvalidity, 0);
        assert!(result.changes.is_empty());
        assert!(result.missing_remote_ids.is_empty());
        assert_eq!(result.folder_error.as_deref(), Some("no uidvalidity"));
    }
}
