//! 将 RFC822 字节解析为 RemoteMessage

use crate::domain::mail::{RemoteFlags, RemoteMessage, RemoteRecipient};
use crate::domain::value_objects::{
    BodyContentType, EmailAddress, FolderId, RemoteKey, Importance, RecipientRole,
};
use mailparse::{parse_mail, MailHeaderMap};

pub fn remote_id(uidvalidity: u32, uid: u32) -> RemoteKey {
    RemoteKey::new(format!("imap:{uidvalidity}:{uid}"))
}

/// Only the IMAP adapter interprets the opaque remote key.
pub fn decode_remote_key(key: &RemoteKey) -> Result<(u32, u32), crate::infrastructure::error::InfraError> {
    let invalid = || crate::infrastructure::error::InfraError::Conflict { message: "IMAP 定位无效，请重新同步".into() };
    let mut parts = key.as_str().split(':');
    if parts.next() != Some("imap") { return Err(invalid()); }
    let generation = parts.next().and_then(|s| s.parse::<u32>().ok()).filter(|v| *v > 0).ok_or_else(invalid)?;
    let uid = parts.next().and_then(|s| s.parse::<u32>().ok()).filter(|v| *v > 0).ok_or_else(invalid)?;
    if parts.next().is_some() { return Err(invalid()); }
    Ok((generation, uid))
}

pub fn parse_rfc822(
    folder: &FolderId,
    uidvalidity: u32,
    uid: u32,
    bytes: &[u8],
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
    let attachments = collect_attachments(&parsed);
    let has_attachments = !attachments.is_empty();
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

    // sent_at 取 Date；received_at 优先 INTERNALDATE，缺失回退 Date
    let header_date = headers
        .get_first_value("Date")
        .and_then(|d| mailparse::dateparse(&d).ok())
        .map(|secs| secs.saturating_mul(1000));
    let sent_at = header_date;
    let received_at = internal_date_ms.or(header_date);

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
        received_at,
        sent_at,
        flags: RemoteFlags::default(),
        has_attachments,
        attachments,
        importance: Importance::Normal,
        folder_id: Some(folder.clone()),
        parent_folder_name: None,
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
                match addr {
                    mailparse::MailAddr::Single(info) => {
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
                    // RFC 5322 地址组：展开组内成员，组名不参与身份
                    mailparse::MailAddr::Group(group) => {
                        for info in &group.addrs {
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
    // 带附件 disposition 的 text/html 属于附件，不能当正文
    if is_attachment_part(parsed) || is_message_part(parsed) {
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

fn is_attachment_part(parsed: &mailparse::ParsedMail<'_>) -> bool {
    matches!(
        parsed.get_content_disposition().disposition,
        mailparse::DispositionType::Attachment
    ) || parsed.ctype.params.contains_key("name")
        || parsed
            .headers
            .get_first_value("Content-Disposition")
            .is_some_and(|v| v.to_ascii_lowercase().contains("filename"))
}

fn is_message_part(parsed: &mailparse::ParsedMail<'_>) -> bool {
    parsed
        .ctype
        .mimetype
        .eq_ignore_ascii_case("message/rfc822")
}

/// 可下载附件：普通附件与带 CID 的 inline 内容；正文 part 不算附件。
fn collect_attachments(parsed: &mailparse::ParsedMail<'_>) -> Vec<crate::domain::mail::RemoteAttachment> {
    let mut out = Vec::new();
    collect_attachments_into(parsed, &mut out);
    out
}

fn collect_attachments_into(
    parsed: &mailparse::ParsedMail<'_>,
    out: &mut Vec<crate::domain::mail::RemoteAttachment>,
) {
    if !parsed.subparts.is_empty() {
        for part in &parsed.subparts {
            collect_attachments_into(part, out);
        }
        return;
    }
    if is_message_part(parsed) {
        return;
    }
    let mime = parsed.ctype.mimetype.to_ascii_lowercase();
    let content_id = parsed
        .headers
        .get_first_value("Content-ID")
        .map(|v| v.trim().trim_matches(|c| c == '<' || c == '>').to_string())
        .filter(|v| !v.is_empty());
    let is_text_body = (mime.starts_with("text/plain") || mime.contains("html")) && content_id.is_none();
    if is_text_body && !is_attachment_part(parsed) {
        return;
    }
    let name = parsed
        .get_content_disposition()
        .params
        .get("filename")
        .cloned()
        .or_else(|| parsed.ctype.params.get("name").cloned())
        .unwrap_or_else(|| {
            content_id
                .clone()
                .unwrap_or_else(|| "attachment".to_string())
        });
    let Some(bytes) = parsed.get_body_raw().ok() else {
        return;
    };
    out.push(crate::domain::mail::RemoteAttachment {
        name,
        mime: parsed.ctype.mimetype.clone(),
        bytes,
        inline: content_id.is_some(),
        content_id,
    });
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

#[cfg(test)]
mod mime_tests {
    use super::*;

    fn parse(raw: &str, internal: Option<i64>) -> crate::domain::mail::RemoteMessage {
        parse_rfc822(&FolderId::new("inbox"), 7, 1, raw.as_bytes(), internal).expect("parse")
    }

    #[test]
    fn text_attachment_is_not_body_and_sets_has_attachments() {
        let raw = concat!(
            "From: a@example.com\r\n",
            "To: b@example.com\r\n",
            "Subject: s\r\n",
            "Date: Mon, 02 Jan 2006 15:04:05 +0000\r\n",
            "MIME-Version: 1.0\r\n",
            "Content-Type: multipart/mixed; boundary=\"b1\"\r\n\r\n",
            "--b1\r\nContent-Type: text/plain\r\n\r\nbody text\r\n",
            "--b1\r\nContent-Type: text/plain; name=\"note.txt\"\r\n",
            "Content-Disposition: attachment; filename=\"note.txt\"\r\n\r\nattached\r\n",
            "--b1--\r\n"
        );
        let msg = parse(raw, None);
        assert!(msg.body_content.as_deref().unwrap_or("").contains("body text"));
        assert!(msg.has_attachments);
        assert_eq!(msg.attachments.len(), 1);
        assert_eq!(msg.attachments[0].name, "note.txt");
        assert_eq!(String::from_utf8_lossy(&msg.attachments[0].bytes).trim(), "attached");
    }

    #[test]
    fn internal_date_preferred_for_received_and_date_for_sent() {
        let raw = concat!(
            "From: a@example.com\r\n",
            "Subject: s\r\n",
            "Date: Mon, 02 Jan 2006 15:04:05 +0000\r\n",
            "Content-Type: text/plain\r\n\r\nhi\r\n"
        );
        let msg = parse(raw, Some(1_000));
        assert_eq!(msg.received_at, Some(1_000));
        assert!(msg.sent_at.is_some());
        assert_ne!(msg.sent_at, Some(1_000));
    }

    #[test]
    fn address_group_is_expanded() {
        let raw = concat!(
            "From: a@example.com\r\n",
            "To: Team: c@example.com, d@example.com;\r\n",
            "Subject: s\r\n",
            "Content-Type: text/plain\r\n\r\nhi\r\n"
        );
        let msg = parse(raw, None);
        let to: Vec<_> = msg
            .recipients
            .iter()
            .filter(|r| r.role == RecipientRole::To)
            .map(|r| r.normalized_address.as_str().to_string())
            .collect();
        assert_eq!(to, vec!["c@example.com", "d@example.com"]);
    }
}
