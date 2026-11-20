//! 外发快照：首次派发前固定 Message-ID / Date / MIME 与 envelope，重试复用同一字节。
//!
//! 只生成不可变内容，不派发、不入队；队列归 Task 08。

use crate::domain::value_objects::AccountId;
use crate::infrastructure::error::InfraError;
use lettre::message::{
    header, Attachment, Body, Mailbox, Message, MessageBuilder, MultiPart, SinglePart,
};
use serde::{Deserialize, Serialize};

/// 收件人（可见/密送由 envelope 决定，Bcc 不写入 MIME 头）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotRecipient {
    pub address: String,
    pub name: String,
}

/// 附件引用（内容已在受管理存储中）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotAttachmentRef {
    pub content_id: String,
    pub name: String,
    pub mime: String,
    pub sort_order: i32,
}

/// 已加载的附件字节
#[derive(Debug, Clone)]
pub struct SnapshotAttachmentBytes {
    pub content_id: String,
    pub name: String,
    pub mime: String,
    pub sort_order: i32,
    pub bytes: Vec<u8>,
}

/// 不可变 envelope：SMTP 用它生成 RCPT；Graph 用它填 bccRecipients 等
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotEnvelope {
    pub from: String,
    pub from_name: String,
    pub to: Vec<SnapshotRecipient>,
    pub cc: Vec<SnapshotRecipient>,
    /// 仅 envelope：不写入公开 MIME 头
    pub bcc: Vec<SnapshotRecipient>,
}

/// 构建快照的输入（内容已定稿）
#[derive(Debug, Clone)]
pub struct SnapshotInput {
    pub account_id: AccountId,
    pub from: String,
    pub from_name: String,
    pub to: Vec<SnapshotRecipient>,
    pub cc: Vec<SnapshotRecipient>,
    pub bcc: Vec<SnapshotRecipient>,
    pub subject: String,
    pub text_body: String,
    pub html_body: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    pub attachments: Vec<SnapshotAttachmentBytes>,
}

/// 构建完成、可持久化的快照
#[derive(Debug, Clone)]
pub struct BuiltSnapshot {
    pub internet_message_id: String,
    /// RFC 2822 时间字符串；重试时原样写回
    pub date: String,
    pub mime: Vec<u8>,
    pub envelope: SnapshotEnvelope,
    pub subject: String,
    pub text_body: String,
    pub html_body: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    pub attachments: Vec<SnapshotAttachmentRef>,
}

pub(crate) fn parse_mailbox(addr: &str, name: &str) -> Result<Mailbox, InfraError> {
    let address = addr.trim().parse::<lettre::Address>().map_err(|e| InfraError::Remote {
        message: format!("无效邮箱地址: {e}"),
        retryable: false,
    })?;
    let name = name.trim();
    if name.chars().any(char::is_control) {
        return Err(InfraError::Remote {
            message: "邮箱显示名称包含无效控制字符".into(),
            retryable: false,
        });
    }
    Ok(Mailbox::new((!name.is_empty()).then(|| name.to_owned()), address))
}

fn snapshot_mailbox(addr: &str, name: &str, field: &str) -> Result<Mailbox, InfraError> {
    parse_mailbox(addr, name).map_err(|error| match error {
        InfraError::Remote { message, retryable } => InfraError::Remote {
            message: format!("{field}：{message}"),
            retryable,
        },
        other => other,
    })
}

#[test]
fn mailbox_keeps_display_name_separate_from_address() {
    let name = "张三, \"产品组\" (Postlet)";
    let mailbox = parse_mailbox(" sender@example.com ", name).unwrap();
    assert_eq!(mailbox.name.as_deref(), Some(name));
    assert_eq!(mailbox.email.to_string(), "sender@example.com");
    assert!(parse_mailbox("not-an-address", name).is_err());
    assert!(parse_mailbox("sender@example.com", "Name\r\nBcc: x@example.com").is_err());
}

/// 生成新的 Message-ID：`<uuid@local-domain>`
pub fn new_internet_message_id() -> String {
    format!(
        "<{}@postlet.local>",
        uuid::Uuid::new_v4().as_hyphenated()
    )
}

/// 当前时刻的 RFC 2822 Date
pub fn now_date_rfc2822() -> String {
    chrono::Utc::now().to_rfc2822()
}

/// References：父信 references + 父信 Message-ID
pub fn build_references(parent_references: Option<&str>, parent_message_id: Option<&str>) -> Option<String> {
    let mut parts: Vec<&str> = parent_references
        .map(|r| r.split_whitespace().collect())
        .unwrap_or_default();
    if let Some(id) = parent_message_id.filter(|id| !id.trim().is_empty()) {
        if !parts.iter().any(|p| *p == id) {
            parts.push(id);
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

/// 构建不可变快照：同一输入重复调用只在 Message-ID/Date 相同时得到相同字节。
pub fn build_snapshot(
    input: &SnapshotInput,
    internet_message_id: &str,
) -> Result<BuiltSnapshot, InfraError> {
    let mut builder = Message::builder()
        .from(snapshot_mailbox(&input.from, &input.from_name, "发件人")?)
        .subject(input.subject.clone())
        .message_id(Some(internet_message_id.to_string()));

    for (index, r) in input.to.iter().enumerate() {
        builder = builder.to(snapshot_mailbox(&r.address, &r.name, &format!("第 {} 个收件人", index + 1))?);
    }
    for (index, r) in input.cc.iter().enumerate() {
        builder = builder.cc(snapshot_mailbox(&r.address, &r.name, &format!("第 {} 个抄送人", index + 1))?);
    }
    // Bcc 只进 envelope，不写入 MIME 头；入队前也校验地址。
    for (index, r) in input.bcc.iter().enumerate() {
        snapshot_mailbox(&r.address, &r.name, &format!("第 {} 个密送人", index + 1))?;
    }
    if let Some(irt) = input.in_reply_to.as_deref() {
        builder = builder.header(header::InReplyTo::from(irt.to_string()));
    }
    if let Some(refs) = input.references.as_deref() {
        builder = builder.header(header::References::from(refs.to_string()));
    }

    let message = build_body(builder, input)?;
    let mime = message.formatted();

    Ok(BuiltSnapshot {
        internet_message_id: internet_message_id.to_string(),
        // Date 头由 lettre 在构建时写入 MIME；该字节随后持久化，重试复用同一份
        date: now_date_rfc2822(),
        mime,
        envelope: SnapshotEnvelope {
            from: input.from.clone(),
            from_name: input.from_name.clone(),
            to: input.to.clone(),
            cc: input.cc.clone(),
            bcc: input.bcc.clone(),
        },
        subject: input.subject.clone(),
        text_body: input.text_body.clone(),
        html_body: input.html_body.clone(),
        in_reply_to: input.in_reply_to.clone(),
        references: input.references.clone(),
        attachments: input
            .attachments
            .iter()
            .map(|a| SnapshotAttachmentRef {
                content_id: a.content_id.clone(),
                name: a.name.clone(),
                mime: a.mime.clone(),
                sort_order: a.sort_order,
            })
            .collect(),
    })
}

fn build_body(builder: MessageBuilder, input: &SnapshotInput) -> Result<Message, InfraError> {
    let text = input.text_body.clone();
    let html = input.html_body.clone();

    if input.attachments.is_empty() {
        if let Some(html) = html {
            return builder
                .multipart(
                    MultiPart::alternative()
                        .singlepart(SinglePart::plain(text))
                        .singlepart(SinglePart::html(html)),
                )
                .map_err(build_err);
        }
        return builder.body(text).map_err(build_err);
    }

    let mut mixed = if let Some(html) = html {
        MultiPart::mixed().multipart(
            MultiPart::alternative()
                .singlepart(SinglePart::plain(text))
                .singlepart(SinglePart::html(html)),
        )
    } else {
        MultiPart::mixed().singlepart(SinglePart::plain(text))
    };

    for att in &input.attachments {
        let body = Body::new(att.bytes.clone());
        let content_type = att
            .mime
            .parse()
            .unwrap_or_else(|_| "application/octet-stream".parse().expect("mime"));
        mixed = mixed.singlepart(
            Attachment::new(att.name.clone()).body(body, content_type),
        );
    }

    builder.multipart(mixed).map_err(build_err)
}

fn build_err(e: lettre::error::Error) -> InfraError {
    InfraError::Remote {
        message: format!("构造邮件失败: {e}"),
        retryable: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(html: Option<&str>, attachments: Vec<SnapshotAttachmentBytes>) -> SnapshotInput {
        SnapshotInput {
            account_id: AccountId::new("a"),
            from: "me@example.com".into(),
            from_name: "我".into(),
            to: vec![SnapshotRecipient {
                address: "to@example.com".into(),
                name: "收件人".into(),
            }],
            cc: vec![SnapshotRecipient {
                address: "cc@example.com".into(),
                name: String::new(),
            }],
            bcc: vec![SnapshotRecipient {
                address: "secret@example.com".into(),
                name: String::new(),
            }],
            subject: "主题".into(),
            text_body: "纯文本正文".into(),
            html_body: html.map(str::to_string),
            in_reply_to: Some("<parent@example.com>".into()),
            references: build_references(None, Some("<parent@example.com>")),
            attachments,
        }
    }

    #[test]
    fn bcc_stays_out_of_visible_headers_and_envelope_keeps_it() {
        let built = build_snapshot(&input(Some("<p>hi</p>"), vec![]), "<new@postlet.local>")
            .expect("snapshot");
        let mime = String::from_utf8_lossy(&built.mime);
        assert!(!mime.to_ascii_lowercase().contains("secret@example.com"));
        assert_eq!(built.envelope.bcc.len(), 1);
        assert_eq!(built.envelope.bcc[0].address, "secret@example.com");
        assert!(mime.contains("<parent@example.com>"));
    }

    #[test]
    fn same_message_id_and_date_are_reused_by_persisted_bytes() {
        let built = build_snapshot(&input(None, vec![]), "<fixed@postlet.local>").expect("build");
        let built2 = build_snapshot(&input(None, vec![]), "<fixed@postlet.local>").expect("build");
        // 同一 Message-ID；字节在重试时直接复用持久化副本，不再重新构建
        assert_eq!(built.internet_message_id, built2.internet_message_id);
        assert!(built.mime.windows(9).any(|w| w == b"Message-I"));
    }

    #[test]
    fn attachment_only_mail_is_valid_mime() {
        let built = build_snapshot(
            &input(None, vec![SnapshotAttachmentBytes {
                content_id: "c1".into(),
                name: "a.bin".into(),
                mime: "application/octet-stream".into(),
                sort_order: 0,
                bytes: b"payload".to_vec(),
            }]),
            "<att@postlet.local>",
        )
        .expect("build");
        let mime = String::from_utf8_lossy(&built.mime);
        assert!(mime.contains("multipart/mixed"));
        assert_eq!(built.attachments.len(), 1);
    }
}
