//! 将 RFC822 字节解析为 RemoteMessage

use crate::domain::mail::{RemoteMessage, RemoteRecipient};
use crate::domain::value_objects::{
    BodyContentType, EmailAddress, FolderId, RemoteKey, Importance, RecipientRole,
};
use mailparse::{parse_mail, MailHeaderMap};

pub fn remote_id(uidvalidity: u32, uid: u32) -> RemoteKey {
    RemoteKey::new(format!("imap:{uidvalidity}:{uid}"))
}

/// Only the IMAP adapter interprets the opaque remote key.
pub fn decode_remote_key(key: &RemoteKey) -> Result<(u32, u32), crate::application::error::PortError> {
    let invalid = || crate::application::error::PortError::Conflict { message: "IMAP 定位无效，请重新同步".into() };
    let mut parts = key.as_str().split(':');
    if parts.next() != Some("imap") { return Err(invalid()); }
    let generation = parts.next().and_then(|s| s.parse::<u32>().ok()).filter(|v| *v > 0).ok_or_else(invalid)?;
    let uid = parts.next().and_then(|s| s.parse::<u32>().ok()).filter(|v| *v > 0).ok_or_else(invalid)?;
    if parts.next().is_some() { return Err(invalid()); }
    Ok((generation, uid))
}

pub fn parse_rfc822(
    folder: &str,
    uidvalidity: u32,
    uid: u32,
    bytes: &[u8],
    is_read: bool,
    internal_date_ms: Option<i64>,
) -> Result<RemoteMessage, String> {
    let parsed = parse_mail(bytes).map_err(|e| format!("mailparse: {e}"))?;
    let headers = &parsed.headers;
    let subject = headers.get_first_value("Subject").unwrap_or_default();
    let message_id = headers.get_first_value("Message-ID");
    let in_reply_to = headers
        .get_first_value("In-Reply-To")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let from_raw = headers.get_first_value("From").unwrap_or_default();
    let (from_name, from_address) = split_mailbox(&from_raw);

    let mut recipients = Vec::new();
    push_role(&mut recipients, RecipientRole::To, headers.get_all_values("To"));
    push_role(&mut recipients, RecipientRole::Cc, headers.get_all_values("Cc"));
    push_role(
        &mut recipients,
        RecipientRole::Bcc,
        headers.get_all_values("Bcc"),
    );
    push_role(
        &mut recipients,
        RecipientRole::ReplyTo,
        headers.get_all_values("Reply-To"),
    );

    let (plain, html) = collect_text_parts(&parsed);
    // 阅读窗：有 HTML 用 HTML；气泡预览：优先 plain，否则去标签
    let (body_content_type, body_content) = if let Some(h) = html.clone() {
        (BodyContentType::Html, h)
    } else {
        (BodyContentType::Text, plain.clone().unwrap_or_default())
    };
    let preview_source = plain
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| html.map(|h| html_to_plain(&h)).unwrap_or_default());
    let preview = truncate_preview(&preview_source);

    let header_date = headers
        .get_first_value("Date")
        .and_then(|d| mailparse::dateparse(&d).ok())
        .map(|secs| secs.saturating_mul(1000));
    let dated = header_date.or(internal_date_ms);

    Ok(RemoteMessage {
        remote_key: remote_id(uidvalidity, uid),
        internet_message_id: message_id,
        in_reply_to,
        subject,
        body_preview: preview,
        body_content_type,
        body_content: Some(body_content),
        from_address,
        from_name,
        recipients,
        received_at: dated,
        sent_at: dated,
        is_read,
        is_draft: false,
        has_attachments: false,
        importance: Importance::Normal,
        folder_id: Some(FolderId::new(folder.to_string())),
        parent_folder_name: Some(folder.to_string()),
        remote_updated_at: None,
    })
}

fn truncate_preview(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .take(180)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// 气泡/列表用：去标签、解码常见实体、压空白。不引入 HTML 引擎。
pub fn html_to_plain(html: &str) -> String {
    let mut s = html.to_string();
    // 丢掉脚本/样式整块
    for tag in ["script", "style", "noscript"] {
        loop {
            let open = format!("<{tag}");
            let close = format!("</{tag}>");
            let Some(start) = s.to_ascii_lowercase().find(&open) else {
                break;
            };
            let rest = &s[start..];
            let Some(rel_end) = rest.to_ascii_lowercase().find(&close) else {
                s.truncate(start);
                break;
            };
            let end = start + rel_end + close.len();
            s.replace_range(start..end, " ");
        }
    }
    s = s
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("</p>", "\n")
        .replace("</div>", "\n")
        .replace("</tr>", "\n")
        .replace("</li>", "\n");
    // 去标签
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    decode_basic_entities(&out)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn decode_basic_entities(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

fn split_mailbox(raw: &str) -> (String, Option<EmailAddress>) {
    if let Ok(addrs) = mailparse::addrparse(raw) {
        if let Some(mailparse::MailAddr::Single(info)) = addrs.first() {
            let name = info.display_name.clone().unwrap_or_default();
            let addr = EmailAddress::parse(&info.addr);
            return (name, addr);
        }
    }
    let trimmed = raw.trim();
    if let Some(addr) = EmailAddress::parse(trimmed) {
        return (String::new(), Some(addr));
    }
    (trimmed.to_string(), None)
}

fn push_role(out: &mut Vec<RemoteRecipient>, role: RecipientRole, values: Vec<String>) {
    let mut seq = out.len() as i32;
    for value in values {
        if let Ok(addrs) = mailparse::addrparse(&value) {
            for addr in addrs.iter() {
                if let mailparse::MailAddr::Single(info) = addr {
                    if let Some(normalized) = EmailAddress::parse(&info.addr) {
                        out.push(RemoteRecipient {
                            role,
                            seq,
                            normalized_address: normalized,
                            display_name: info.display_name.clone().unwrap_or_default(),
                        });
                        seq += 1;
                    }
                }
            }
        }
    }
}

/// 递归收集 text/plain 与 text/html（multipart 嵌套也能拿到）。
fn collect_text_parts(parsed: &mailparse::ParsedMail<'_>) -> (Option<String>, Option<String>) {
    let mut plain = None;
    let mut html = None;
    walk_parts(parsed, &mut plain, &mut html);
    (plain, html)
}

fn walk_parts(
    parsed: &mailparse::ParsedMail<'_>,
    plain: &mut Option<String>,
    html: &mut Option<String>,
) {
    if !parsed.subparts.is_empty() {
        for part in &parsed.subparts {
            walk_parts(part, plain, html);
        }
        return;
    }
    let mime = parsed.ctype.mimetype.to_ascii_lowercase();
    let body = parsed.get_body().unwrap_or_default();
    if mime.starts_with("text/plain") && plain.is_none() {
        *plain = Some(body);
    } else if mime.contains("html") && html.is_none() {
        *html = Some(body);
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;

    #[test]
    fn uid_is_scoped_to_generation_and_malformed_targets_are_rejected() {
        assert_ne!(remote_id(1, 10), remote_id(2, 10));
        assert_eq!(decode_remote_key(&remote_id(u32::MAX, u32::MAX)).unwrap(), (u32::MAX, u32::MAX));
        for invalid in ["imap:INBOX:10", "imap:0:10", "imap:1:0", "imap:1:2:3", "10", "imap:4294967296:1"] {
            assert!(decode_remote_key(&RemoteKey::new(invalid)).is_err());
        }
    }
}
