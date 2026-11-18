//! Microsoft Graph 邮件协议：Delta / 正文 / 已读 PATCH / 发送。
//!
//! 只做远端 IO + 映射到 `RemoteMessage`；本地落库与聚合由 application 编排。

use crate::domain::mail::{RemoteFlags, RemoteMessageChange, RemoteMessage, RemoteRecipient};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailDeltaPage {
    pub messages: Vec<RemoteMessageChange>,
    pub next_link: Option<String>,
    pub delta_link: Option<String>,
}

use crate::infrastructure::error::InfraError;
use crate::domain::value_objects::{
    BodyContentType, EmailAddress, FolderId, RemoteKey, Importance, RecipientRole,
};
use chrono::{DateTime, Utc};
use graph_rs_sdk::{GraphClient, ODataQuery};
use serde::Deserialize;
use serde_json::{json, Value};

const MESSAGE_SELECT: &[&str] = &[
    "id",
    "internetMessageId",
    "internetMessageHeaders",
    "subject",
    "bodyPreview",
    "body",
    "from",
    "toRecipients",
    "ccRecipients",
    "bccRecipients",
    "replyTo",
    "receivedDateTime",
    "sentDateTime",
    "lastModifiedDateTime",
    "isRead",
    "isDraft",
    "hasAttachments",
    "importance",
    "parentFolderId",
];

/// Graph 外发载荷（由 application 从本地 Message 组装）。
#[derive(Debug, Clone)]
pub struct GraphOutgoing {
    pub subject: String,
    pub body_content_type: BodyContentType,
    pub body_content: String,
    pub to: Vec<(String, String)>,
    pub cc: Vec<(String, String)>,
    pub bcc: Vec<(String, String)>,
    pub save_to_sent_items: bool,
}

#[derive(Debug, Deserialize)]
struct ODataList {
    value: Vec<Value>,
    #[serde(rename = "@odata.nextLink")]
    next_link: Option<String>,
    #[serde(rename = "@odata.deltaLink")]
    delta_link: Option<String>,
}

fn client(access_token: &str) -> GraphClient {
    GraphClient::new(access_token)
}

fn map_http_err(context: &str, err: impl std::fmt::Display) -> InfraError {
    InfraError::Network {
        message: format!("{context}: {err}"),
    }
}

async fn ensure_success(
    response: reqwest::Response,
    context: &str,
) -> Result<reqwest::Response, InfraError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    Err(classify_graph_status(status, context, &body))
}

/// Graph 结果分类：401/403 视为认证问题，4xx/429 暂时，其余不可自动重试。
fn classify_graph_status(
    status: reqwest::StatusCode,
    context: &str,
    body: &str,
) -> InfraError {
    match status.as_u16() {
        401 | 403 => InfraError::Auth {
            message: format!("{context} 鉴权被拒 ({status}): {body}"),
        },
        429 => InfraError::Remote {
            message: format!("{context} 限流 ({status}): {body}"),
            retryable: true,
        },
        400..=499 => InfraError::Remote {
            message: format!("{context} 被拒绝 ({status}): {body}"),
            retryable: false,
        },
        500..=599 => InfraError::Remote {
            message: format!("{context} 服务端错误 ({status}): {body}"),
            retryable: true,
        },
        _ => InfraError::Remote {
            message: format!("{context} 失败 ({status}): {body}"),
            retryable: false,
        },
    }
}

/// 文件夹消息 Delta 一页；`delta_or_next_link` 为绝对 URL 时直接续页。
pub async fn delta_messages(
    access_token: &str,
    folder_id: &FolderId,
    remote_binding: &str,
    delta_or_next_link: Option<&str>,
) -> Result<MailDeltaPage, InfraError> {
    let response = if let Some(link) = delta_or_next_link.filter(|s| !s.is_empty()) {
        get_absolute(access_token, link).await?
    } else {
        let folder = remote_binding;
        let response = client(access_token)
            .me()
            .mail_folder(folder)
            .messages()
            .delta()
            .select(MESSAGE_SELECT)
            .send()
            .await
            .map_err(|e| map_http_err("Graph messages delta", e))?;
        ensure_success(response, "Graph messages delta").await?
    };

    let page: ODataList = response.json().await.map_err(|_| InfraError::Remote {
        message: "无法解析 Graph Delta 响应".into(),
        retryable: true,
    })?;

    let mut messages = Vec::with_capacity(page.value.len());
    for mut item in page.value {
        if item.get("@removed").is_some() {
            if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                messages.push(RemoteMessageChange::Remove {
                    remote_key: RemoteKey::new(id.to_string()),
                });
            }
            continue;
        }
        if needs_full_snapshot(&item) {
            let id = item.get("id").and_then(Value::as_str).ok_or_else(|| InfraError::Remote {
                message: "Graph 变更缺少定位键".into(), retryable: false,
            })?.to_owned();
            // Hydrate only within this source folder; never follow an ID into another location.
            let response = client(access_token).me().mail_folder(remote_binding).messages_id(&id)
                .get_messages().select(MESSAGE_SELECT).send().await
                .map_err(|error| map_http_err("Graph hydrate snapshot", error))?;
            if response.status().as_u16() == 404 {
                messages.push(RemoteMessageChange::Remove { remote_key: RemoteKey::new(id) });
                continue;
            }
            item = ensure_success(response, "Graph hydrate snapshot").await?.json().await
                .map_err(|_| InfraError::Remote { message: "Graph 完整快照无法解析".into(), retryable: true })?;
            if needs_full_snapshot(&item) {
                return Err(InfraError::Remote { message: "Graph 未返回完整邮件快照，已停止覆盖本地内容".into(), retryable: true });
            }
        }
        match map_remote_message(&item, folder_id.clone()) {
            Ok(remote) => messages.push(RemoteMessageChange::Upsert(remote)),
            Err(err) => {
                return Err(InfraError::Remote {
                    message: format!("映射 Graph 邮件失败: {err}"),
                    retryable: false,
                });
            }
        }
    }

    Ok(MailDeltaPage {
        messages,
        next_link: page.next_link,
        delta_link: page.delta_link,
    })
}

pub async fn get_message_body(
    access_token: &str,
    remote_binding: &str,
    remote_key: &RemoteKey,
) -> Result<(BodyContentType, String), InfraError> {
    let response = client(access_token)
        .me()
        .mail_folder(remote_binding).messages_id(remote_key.as_str())
        .get_messages()
        .select(&["id", "body"])
        .send()
        .await
        .map_err(|e| map_http_err("Graph get body", e))?;
    let response = ensure_success(response, "Graph get body").await?;
    let value: Value = response.json().await.map_err(|_| InfraError::Remote {
        message: "无法解析 Graph 正文".into(),
        retryable: true,
    })?;
    let body = value.get("body").ok_or_else(|| InfraError::Remote {
        message: "Graph 邮件缺少 body".into(),
        retryable: true,
    })?;
    let content_type = match body
        .get("contentType")
        .and_then(|v| v.as_str())
        .unwrap_or("text")
    {
        s if s.eq_ignore_ascii_case("html") => BodyContentType::Html,
        _ => BodyContentType::Text,
    };
    let content = body
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    Ok((content_type, content))
}

pub async fn patch_message_read(
    access_token: &str,
    remote_binding: &str,
    remote_key: &RemoteKey,
    is_read: bool,
) -> Result<(), InfraError> {
    let response = client(access_token)
        .me()
        .mail_folder(remote_binding).messages_id(remote_key.as_str())
        .update_messages(&json!({ "isRead": is_read }))
        .send()
        .await
        .map_err(|e| map_http_err("Graph patch isRead", e))?;
    ensure_success(response, "Graph patch isRead").await?;
    Ok(())
}

/// 删除邮件（通常进入「已删除邮件」）。
pub async fn delete_message(
    access_token: &str,
    remote_binding: &str,
    remote_key: &RemoteKey,
) -> Result<(), InfraError> {
    let response = client(access_token)
        .me()
        .mail_folder(remote_binding).messages_id(remote_key.as_str())
        .delete_messages()
        .send()
        .await
        .map_err(|e| map_http_err("Graph delete message", e))?;
    ensure_success(response, "Graph delete message").await?;
    Ok(())
}

/// 将消息移动到当前账号内的目标文件夹（Graph move 为原子移动）
pub async fn move_message(
    access_token: &str,
    remote_binding: &str,
    remote_key: &RemoteKey,
    destination_folder_id: &str,
) -> Result<(), InfraError> {
    let payload = json!({ "destinationId": destination_folder_id });
    let response = client(access_token)
        .me()
        .mail_folder(remote_binding)
        .messages_id(remote_key.as_str())
        .move_message(&payload)
        .send()
        .await
        .map_err(|e| map_http_err("Graph move message", e))?;
    ensure_success(response, "Graph move message").await?;
    Ok(())
}

pub async fn send_mail(access_token: &str, mail: &GraphOutgoing) -> Result<(), InfraError> {
    let content_type = match mail.body_content_type {
        BodyContentType::Html => "HTML",
        BodyContentType::Text => "Text",
    };
    let payload = json!({
        "message": {
            "subject": mail.subject,
            "body": {
                "contentType": content_type,
                "content": mail.body_content,
            },
            "toRecipients": recipients_json(&mail.to),
            "ccRecipients": recipients_json(&mail.cc),
            "bccRecipients": recipients_json(&mail.bcc),
        },
        "saveToSentItems": mail.save_to_sent_items,
    });

    let response = client(access_token)
        .me()
        .send_mail(&payload)
        .send()
        .await
        .map_err(|e| map_http_err("Graph sendMail", e))?;
    ensure_success(response, "Graph sendMail").await?;
    Ok(())
}

fn recipients_json(list: &[(String, String)]) -> Vec<Value> {
    list.iter()
        .map(|(address, name)| {
            let mut email = json!({ "address": address });
            if !name.trim().is_empty() {
                email["name"] = Value::String(name.clone());
            }
            json!({ "emailAddress": email })
        })
        .collect()
}

/// 远端邮件文件夹（含父子；多级）
#[derive(Debug, Clone)]
pub struct RemoteMailFolder {
    pub id: String,
    pub display_name: String,
    pub parent_id: Option<String>,
    /// Graph 专有 well-known（仅本层使用）
    pub well_known_name: Option<String>,
}

/// Graph well-known → 统一 IMAP 对齐角色；未知返回 None。
pub fn role_of_well_known(name: &str) -> Option<crate::domain::services::FolderRole> {
    use crate::domain::services::FolderRole;
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "inbox" => FolderRole::Inbox,
        "sentitems" => FolderRole::Sent,
        "drafts" => FolderRole::Drafts,
        "deleteditems" => FolderRole::Trash,
        "junkemail" => FolderRole::Junk,
        "archive" => FolderRole::Archive,
        "allitems" => FolderRole::All,
        _ => return None,
    })
}

const FOLDER_SELECT: &[&str] = &["id", "displayName", "parentFolderId", "childFolderCount"];

#[derive(Debug, Clone)]
struct ListedFolder {
    id: String,
    display_name: String,
    child_folder_count: i64,
}

/// 完整枚举目录树（含隐藏绑定），成功后才能据此核对目录退役。
/// 目录刷新不持久化 Delta 进度；这里不叠加第二套发现算法。
pub async fn list_mail_folders(access_token: &str) -> Result<Vec<RemoteMailFolder>, InfraError> {
    let mut out = list_mail_folders_bfs(access_token).await?;
    annotate_inbox_well_known(access_token, &mut out).await;
    Ok(out)
}

async fn annotate_inbox_well_known(access_token: &str, folders: &mut [RemoteMailFolder]) {
    if let Ok(inbox_id) = resolve_well_known_folder_id(access_token, "inbox").await {
        for f in folders.iter_mut() {
            if f.id == inbox_id {
                f.well_known_name = Some("inbox".into());
            }
        }
    }
}

/// 根 + childFolders BFS；仅完整树成功后才允许核对目录退役。
async fn list_mail_folders_bfs(access_token: &str) -> Result<Vec<RemoteMailFolder>, InfraError> {
    use std::collections::{HashSet, VecDeque};

    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut queue: VecDeque<Option<String>> = VecDeque::new();
    queue.push_back(None);

    while let Some(parent_id) = queue.pop_front() {
        let listed = list_folders_under(access_token, parent_id.as_deref()).await?;
        for item in listed {
            if !seen.insert(item.id.clone()) {
                continue;
            }
            let has_children = item.child_folder_count > 0;
            out.push(RemoteMailFolder {
                id: item.id.clone(),
                display_name: item.display_name,
                parent_id: parent_id.clone(),
                well_known_name: None,
            });
            if has_children {
                queue.push_back(Some(item.id));
            }
        }
    }
    Ok(out)
}

/// 用 well-known 名取真实 folder id，例如 `inbox` → opaque id。
async fn resolve_well_known_folder_id(
    access_token: &str,
    well_known: &str,
) -> Result<String, InfraError> {
    let response = client(access_token)
        .me()
        .mail_folder(well_known)
        .get_mail_folders()
        .select(&["id"])
        .send()
        .await
        .map_err(|e| map_http_err("Graph well-known mailFolder", e))?;
    let response = ensure_success(response, "Graph well-known mailFolder").await?;
    let value: Value = response.json().await.map_err(|_| InfraError::Remote {
        message: "无法解析 Graph well-known 文件夹".into(),
        retryable: true,
    })?;
    value
        .get("id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| InfraError::Remote {
            message: format!("well-known 文件夹缺少 id: {well_known}"),
            retryable: false,
        })
}

async fn list_folders_under(
    access_token: &str,
    parent_id: Option<&str>,
) -> Result<Vec<ListedFolder>, InfraError> {
    let mut out = Vec::new();
    let mut next: Option<String> = None;
    loop {
        let response = if let Some(link) = next.as_deref() {
            get_absolute(access_token, link).await?
        } else if let Some(pid) = parent_id {
            let response = client(access_token)
                .me()
                .mail_folder(pid)
                .child_folders()
                .list_child_folders()
                .query(&[("includeHiddenFolders", "true")])
                .select(FOLDER_SELECT)
                .send()
                .await
                .map_err(|e| map_http_err("Graph childFolders", e))?;
            ensure_success(response, "Graph childFolders").await?
        } else {
            let response = client(access_token)
                .me()
                .mail_folders()
                .list_mail_folders()
                .query(&[("includeHiddenFolders", "true")])
                .select(FOLDER_SELECT)
                .send()
                .await
                .map_err(|e| map_http_err("Graph mailFolders", e))?;
            ensure_success(response, "Graph mailFolders").await?
        };

        let page: ODataList = response.json().await.map_err(|_| InfraError::Remote {
            message: "无法解析 Graph 文件夹列表".into(),
            retryable: true,
        })?;

        for item in page.value {
            let id = item
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            if id.is_empty() { return Err(InfraError::Remote { message: "Graph 目录项缺少绑定，未确认完整快照".into(), retryable: true }); }
            let display_name = item
                .get("displayName")
                .and_then(|v| v.as_str())
                .unwrap_or(id.as_str())
                .to_string();
            let child_folder_count = child_count(&item)?;
            out.push(ListedFolder {
                id,
                display_name,
                child_folder_count,
            });
        }

        match page.next_link {
            Some(link) if !link.is_empty() => next = Some(link),
            _ => break,
        }
    }
    Ok(out)
}

fn child_count(value: &Value) -> Result<i64, InfraError> {
    value.get("childFolderCount").and_then(Value::as_i64).filter(|count| *count >= 0)
        .ok_or_else(|| InfraError::Remote { message: "Graph 未返回有效子目录数量，未确认完整目录快照".into(), retryable: true })
}

async fn get_absolute(access_token: &str, link: &str) -> Result<reqwest::Response, InfraError> {
    super::http::get_absolute(access_token, link, "Graph absolute GET").await
}

fn needs_full_snapshot(value: &Value) -> bool {
    ["subject", "body", "from", "toRecipients", "isRead", "isDraft", "hasAttachments", "importance"]
        .iter().any(|field| value.get(*field).is_none())
}

fn map_remote_message(value: &Value, folder_hint: FolderId) -> Result<RemoteMessage, String> {
    let id = value
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "缺少 id".to_string())?;
    let subject = value
        .get("subject")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let body_preview = value
        .get("bodyPreview")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    let (body_content_type, body_content) = match value.get("body") {
        Some(body) => {
            let ct = match body
                .get("contentType")
                .and_then(|v| v.as_str())
                .unwrap_or("text")
            {
                s if s.eq_ignore_ascii_case("html") => BodyContentType::Html,
                _ => BodyContentType::Text,
            };
            let content = body
                .get("content")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            (ct, content)
        }
        None => (BodyContentType::Text, None),
    };

    let (from_name, from_address) = parse_email_address(value.get("from"));
    let mut recipients = Vec::new();
    push_recipients(&mut recipients, RecipientRole::To, value.get("toRecipients"));
    push_recipients(&mut recipients, RecipientRole::Cc, value.get("ccRecipients"));
    push_recipients(&mut recipients, RecipientRole::Bcc, value.get("bccRecipients"));
    push_recipients(&mut recipients, RecipientRole::ReplyTo, value.get("replyTo"));

    // The caller supplies local identity for this folder-scoped request.
    // Graph's parentFolderId is a remote binding, never a local FolderId.
    let folder_id = Some(folder_hint);

    let importance = match value
        .get("importance")
        .and_then(|v| v.as_str())
        .unwrap_or("normal")
    {
        s if s.eq_ignore_ascii_case("low") => Importance::Low,
        s if s.eq_ignore_ascii_case("high") => Importance::High,
        _ => Importance::Normal,
    };

    let in_reply_to = value
        .get("internetMessageHeaders")
        .and_then(|v| v.as_array())
        .and_then(|arr| {
            arr.iter().find_map(|h| {
                let name = h.get("name").and_then(|n| n.as_str())?;
                if name.eq_ignore_ascii_case("In-Reply-To") {
                    h.get("value")
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                } else {
                    None
                }
            })
        });

    Ok(RemoteMessage {
        attachments: Vec::new(),
        remote_key: RemoteKey::new(id.to_string()),
        internet_message_id: value
            .get("internetMessageId")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        in_reply_to,
        subject,
        body_preview,
        body_content_type,
        body_content,
        from_address,
        from_name,
        recipients,
        received_at: parse_graph_time(value.get("receivedDateTime")),
        sent_at: parse_graph_time(value.get("sentDateTime")),
        flags: RemoteFlags {
            is_read: value.get("isRead").and_then(Value::as_bool),
            is_draft: value.get("isDraft").and_then(Value::as_bool),
            is_deleted: None,
        },
        has_attachments: value
            .get("hasAttachments")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        importance,
        folder_id,
        parent_folder_name: None,
        remote_updated_at: parse_graph_time(value.get("lastModifiedDateTime")),
    })
}

fn parse_email_address(node: Option<&Value>) -> (String, Option<EmailAddress>) {
    let Some(from) = node else {
        return (String::new(), None);
    };
    let email = from.get("emailAddress").unwrap_or(from);
    let name = email
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let address = email
        .get("address")
        .and_then(|v| v.as_str())
        .and_then(EmailAddress::parse);
    (name, address)
}

fn push_recipients(out: &mut Vec<RemoteRecipient>, role: RecipientRole, node: Option<&Value>) {
    let Some(Value::Array(items)) = node else {
        return;
    };
    let mut seq = out.len() as i32;
    for item in items {
        let (display_name, normalized_address) = parse_email_address(Some(item));
        let Some(normalized_address) = normalized_address else {
            continue;
        };
        out.push(RemoteRecipient {
            role,
            seq,
            normalized_address,
            display_name,
        });
        seq += 1;
    }
}

fn parse_graph_time(node: Option<&Value>) -> Option<i64> {
    let raw = node?.as_str()?;
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.with_timezone(&Utc).timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_traversal_never_treats_unknown_child_count_as_empty() {
        assert_eq!(child_count(&json!({"childFolderCount": 0})).unwrap(), 0);
        assert_eq!(child_count(&json!({"childFolderCount": 12})).unwrap(), 12);
        for value in [json!({}), json!({"childFolderCount": null}), json!({"childFolderCount": -1}), json!({"childFolderCount": "0"})] {
            assert!(child_count(&value).is_err());
        }
    }

    #[test]
    fn maps_graph_message_json() {
        let value = json!({
            "id": "AAMkAG",
            "changeKey": "ck",
            "internetMessageId": "<a@b.com>",
            "internetMessageHeaders": [
                { "name": "In-Reply-To", "value": "<parent@b.com>" }
            ],
            "subject": "Hello",
            "bodyPreview": "preview",
            "body": { "contentType": "HTML", "content": "<p>hi</p>" },
            "from": { "emailAddress": { "name": "Alice", "address": "a@example.com" } },
            "toRecipients": [
                { "emailAddress": { "name": "Bob", "address": "b@example.com" } }
            ],
            "receivedDateTime": "2024-01-02T03:04:05Z",
            "isRead": true,
            "isDraft": false,
            "hasAttachments": false,
            "importance": "high",
            "parentFolderId": "inbox-id"
        });
        let remote = map_remote_message(&value, FolderId::new("local-folder")).unwrap();
        assert_eq!(remote.remote_key.as_str(), "AAMkAG");
        assert_eq!(remote.subject, "Hello");
        assert_eq!(remote.body_content_type, BodyContentType::Html);
        assert_eq!(remote.from_name, "Alice");
        assert_eq!(remote.recipients.len(), 1);
        assert_eq!(remote.importance, Importance::High);
        assert_eq!(remote.flags.is_read, Some(true));
        assert_eq!(remote.in_reply_to.as_deref(), Some("<parent@b.com>"));
        assert_eq!(remote.folder_id, Some(FolderId::new("local-folder")));
        assert!(!needs_full_snapshot(&value));
        assert!(needs_full_snapshot(&json!({ "id": "AAMkAG", "isRead": false })));
        let mut empty_subject = value.clone(); empty_subject["subject"] = json!("");
        assert!(!needs_full_snapshot(&empty_subject));
        assert_eq!(map_remote_message(&empty_subject, FolderId::new("local-folder")).unwrap().subject, "");
        let mut changed_metadata = value.clone();
        changed_metadata["changeKey"] = json!("different");
        changed_metadata["conversationId"] = json!("unrelated-thread");
        changed_metadata["createdDateTime"] = json!("2000-01-01T00:00:00Z");
        assert_eq!(remote, map_remote_message(&changed_metadata, FolderId::new("local-folder")).unwrap());
    }
}
