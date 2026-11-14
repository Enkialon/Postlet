//! Microsoft Graph 邮件协议：Delta / 正文 / 定位 / 已读 PATCH / 发送。
//!
//! 只做远端 IO + 映射到 `RemoteMessage`；本地落库与聚合由 application 编排。

use crate::application::dto::remote_models::{
    GraphMessageChange, MailDeltaPage, RemoteMessage, RemoteMessageLocation, RemoteRecipient,
};
use crate::application::error::PortError;
use crate::domain::value_objects::{
    BodyContentType, EmailAddress, FolderId, GraphImmutableId, Importance, RecipientRole, ThreadId,
};
use chrono::{DateTime, Utc};
use graph_rs_sdk::http::Method;
use graph_rs_sdk::{GraphClient, ODataQuery};
use serde::Deserialize;
use serde_json::{json, Value};
use url::Url;

const MESSAGE_SELECT: &[&str] = &[
    "id",
    "changeKey",
    "conversationId",
    "internetMessageId",
    "conversationIndex",
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
    "createdDateTime",
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

fn map_http_err(context: &str, err: impl std::fmt::Display) -> PortError {
    PortError::Network {
        message: format!("{context}: {err}"),
    }
}

async fn ensure_success(
    response: reqwest::Response,
    context: &str,
) -> Result<reqwest::Response, PortError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let retryable = status.as_u16() == 429 || status.is_server_error();
    let body = response.text().await.unwrap_or_default();
    Err(PortError::Remote {
        message: format!("{context} 失败 ({status}): {body}"),
        retryable,
    })
}

/// 文件夹消息 Delta 一页；`delta_or_next_link` 为绝对 URL 时直接续页。
pub async fn delta_messages(
    access_token: &str,
    folder_id: &FolderId,
    delta_or_next_link: Option<&str>,
) -> Result<MailDeltaPage, PortError> {
    let response = if let Some(link) = delta_or_next_link.filter(|s| !s.is_empty()) {
        get_absolute(access_token, link).await?
    } else {
        let folder = folder_id.as_str();
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

    let page: ODataList = response.json().await.map_err(|_| PortError::Remote {
        message: "无法解析 Graph Delta 响应".into(),
        retryable: true,
    })?;

    let mut messages = Vec::with_capacity(page.value.len());
    for item in page.value {
        if item.get("@removed").is_some() {
            if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                messages.push(GraphMessageChange::Remove {
                    graph_id: GraphImmutableId::new(id.to_string()),
                });
            }
            continue;
        }
        match map_remote_message(&item, Some(folder_id.clone())) {
            Ok(remote) => messages.push(GraphMessageChange::Upsert(remote)),
            Err(err) => {
                return Err(PortError::Remote {
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

pub async fn get_message_snapshot(
    access_token: &str,
    graph_id: &GraphImmutableId,
) -> Result<Option<RemoteMessage>, PortError> {
    let response = client(access_token)
        .me()
        .message(graph_id.as_str())
        .get_messages()
        .select(MESSAGE_SELECT)
        .send()
        .await
        .map_err(|e| map_http_err("Graph get message", e))?;

    if response.status().as_u16() == 404 {
        return Ok(None);
    }
    let response = ensure_success(response, "Graph get message").await?;
    let value: Value = response.json().await.map_err(|_| PortError::Remote {
        message: "无法解析 Graph 邮件".into(),
        retryable: true,
    })?;
    let remote = map_remote_message(&value, None).map_err(|e| PortError::Remote {
        message: format!("映射 Graph 邮件失败: {e}"),
        retryable: false,
    })?;
    Ok(Some(remote))
}

pub async fn locate_message(
    access_token: &str,
    graph_id: &GraphImmutableId,
) -> Result<RemoteMessageLocation, PortError> {
    match get_message_snapshot(access_token, graph_id).await? {
        None => Ok(RemoteMessageLocation::NotFound),
        Some(remote) => {
            if let Some(folder_id) = remote.folder_id {
                Ok(RemoteMessageLocation::Present { folder_id })
            } else {
                Ok(RemoteMessageLocation::NotFound)
            }
        }
    }
}

pub async fn get_message_body(
    access_token: &str,
    graph_id: &GraphImmutableId,
) -> Result<(BodyContentType, String), PortError> {
    let response = client(access_token)
        .me()
        .message(graph_id.as_str())
        .get_messages()
        .select(&["id", "body"])
        .send()
        .await
        .map_err(|e| map_http_err("Graph get body", e))?;
    let response = ensure_success(response, "Graph get body").await?;
    let value: Value = response.json().await.map_err(|_| PortError::Remote {
        message: "无法解析 Graph 正文".into(),
        retryable: true,
    })?;
    let body = value.get("body").ok_or_else(|| PortError::Remote {
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
    graph_id: &GraphImmutableId,
    is_read: bool,
) -> Result<(), PortError> {
    let response = client(access_token)
        .me()
        .message(graph_id.as_str())
        .update_messages(&json!({ "isRead": is_read }))
        .send()
        .await
        .map_err(|e| map_http_err("Graph patch isRead", e))?;
    ensure_success(response, "Graph patch isRead").await?;
    Ok(())
}

pub async fn send_mail(access_token: &str, mail: &GraphOutgoing) -> Result<(), PortError> {
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

async fn get_absolute(access_token: &str, link: &str) -> Result<reqwest::Response, PortError> {
    let url = Url::parse(link).map_err(|_| PortError::Remote {
        message: format!("无效 Graph 续页链接: {link}"),
        retryable: false,
    })?;
    let mut graph = GraphClient::new(access_token);
    graph.custom_endpoint(&url);
    let response = graph
        .custom(Method::GET, None)
        .send()
        .await
        .map_err(|e| map_http_err("Graph absolute GET", e))?;
    ensure_success(response, "Graph absolute GET").await
}

fn map_remote_message(value: &Value, folder_hint: Option<FolderId>) -> Result<RemoteMessage, String> {
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

    let folder_id = value
        .get("parentFolderId")
        .and_then(|v| v.as_str())
        .map(|s| FolderId::new(s.to_string()))
        .or(folder_hint);

    let importance = match value
        .get("importance")
        .and_then(|v| v.as_str())
        .unwrap_or("normal")
    {
        s if s.eq_ignore_ascii_case("low") => Importance::Low,
        s if s.eq_ignore_ascii_case("high") => Importance::High,
        _ => Importance::Normal,
    };

    let conversation_index = value
        .get("conversationIndex")
        .and_then(|v| v.as_str())
        .and_then(|s| {
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, s).ok()
        });

    Ok(RemoteMessage {
        graph_id: GraphImmutableId::new(id.to_string()),
        thread_id: value
            .get("conversationId")
            .and_then(|v| v.as_str())
            .map(|s| ThreadId::new(s.to_string())),
        graph_change_key: value
            .get("changeKey")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        internet_message_id: value
            .get("internetMessageId")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        conversation_index,
        subject,
        body_preview,
        body_content_type,
        body_content,
        from_address,
        from_name,
        recipients,
        received_at: parse_graph_time(value.get("receivedDateTime")),
        sent_at: parse_graph_time(value.get("sentDateTime")),
        created_at_graph: parse_graph_time(value.get("createdDateTime")),
        is_read: value
            .get("isRead")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        is_draft: value
            .get("isDraft")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
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
    fn maps_graph_message_json() {
        let value = json!({
            "id": "AAMkAG",
            "changeKey": "ck",
            "conversationId": "thread-1",
            "internetMessageId": "<a@b.com>",
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
        let remote = map_remote_message(&value, None).unwrap();
        assert_eq!(remote.graph_id.as_str(), "AAMkAG");
        assert_eq!(remote.subject, "Hello");
        assert_eq!(remote.body_content_type, BodyContentType::Html);
        assert_eq!(remote.from_name, "Alice");
        assert_eq!(remote.recipients.len(), 1);
        assert_eq!(remote.importance, Importance::High);
        assert!(remote.is_read);
    }
}
