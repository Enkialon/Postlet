//! 将 RFC822 字节解析为 RemoteMessage

use crate::application::dto::remote_models::{RemoteMessage, RemoteRecipient};
use crate::domain::value_objects::{
    BodyContentType, EmailAddress, FolderId, GraphImmutableId, Importance, RecipientRole,
};
use mailparse::{parse_mail, MailHeaderMap};

pub fn remote_id(folder: &str, uid: u32) -> GraphImmutableId {
    GraphImmutableId::new(format!("imap:{folder}:{uid}"))
}

pub fn parse_rfc822(
    folder: &str,
    uid: u32,
    bytes: &[u8],
    is_read: bool,
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

    let (body_content_type, body_content) = extract_body(&parsed);
    let preview: String = body_content
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .take(180)
        .collect();

    Ok(RemoteMessage {
        graph_id: remote_id(folder, uid),
        graph_change_key: Some(format!("uid:{uid}")),
        internet_message_id: message_id,
        in_reply_to,
        subject,
        body_preview: preview,
        body_content_type,
        body_content: Some(body_content),
        from_address,
        from_name,
        recipients,
        received_at: None,
        sent_at: None,
        created_at_graph: None,
        is_read,
        is_draft: false,
        has_attachments: false,
        importance: Importance::Normal,
        folder_id: Some(FolderId::new(folder.to_string())),
        parent_folder_name: Some(folder.to_string()),
        remote_updated_at: None,
    })
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

fn extract_body(parsed: &mailparse::ParsedMail<'_>) -> (BodyContentType, String) {
    if parsed.subparts.is_empty() {
        let ctype = parsed
            .ctype
            .mimetype
            .to_ascii_lowercase();
        let body = parsed.get_body().unwrap_or_default();
        if ctype.contains("html") {
            return (BodyContentType::Html, body);
        }
        return (BodyContentType::Text, body);
    }
    let mut text = None;
    let mut html = None;
    for part in &parsed.subparts {
        let mime = part.ctype.mimetype.to_ascii_lowercase();
        let body = part.get_body().unwrap_or_default();
        if mime.contains("html") && html.is_none() {
            html = Some(body);
        } else if mime.starts_with("text/plain") && text.is_none() {
            text = Some(body);
        }
    }
    if let Some(h) = html {
        return (BodyContentType::Html, h);
    }
    (
        BodyContentType::Text,
        text.unwrap_or_default(),
    )
}
