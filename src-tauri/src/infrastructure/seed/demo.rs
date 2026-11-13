//! 演示工作区种子（镜像前端 `seedDemo.ts`，不接 Graph）。

use crate::application::error::PortError;
use crate::domain::entities::{
    Account, Contact, Conversation, ConversationMessageLink, Group, GroupParticipant, MailFolderSyncState,
    Message, MessageRecipient, MessageWithRecipients,
};
use crate::domain::value_objects::{
    AccountId, BodyContentType, ContactId, ConversationId, ConversationKind, EmailAddress, FolderId,
    GraphImmutableId, GroupId, GroupSource, Importance, MessageId, MessageSyncStatus, RecipientRole,
    SyncStatus, ThreadId, TimestampMs,
};
use crate::infrastructure::repository::{
    accounts, contacts, conversations, folder_states, groups, messages,
};
use sqlx::SqliteConnection;

/// 清空全部账户数据并装载演示工作区；返回演示账户。
pub async fn seed_demo_workspace(conn: &mut SqliteConnection) -> Result<Account, PortError> {
    accounts::delete_all(conn).await?;
    let t = now_ms();
    let account_id = AccountId::new("acc-demo");
    let account = Account {
        id: account_id.clone(),
        email: EmailAddress::new_unchecked("me@example.com"),
        display_name: "演示用户".into(),
        aliases: vec![EmailAddress::new_unchecked("alias@example.com")],
        tenant_id: Some("tenant-demo".into()),
        home_account_id: Some("home-demo".into()),
        contact_delta_link: Some("delta-contacts".into()),
        mail_synced_at: Some(t - 60_000),
        contact_synced_at: Some(t - 120_000),
        mail_sync_status: SyncStatus::Idle,
        mail_sync_error: None,
        show_group_messages_in_contact_conversation: false,
        created_at: t - 86_400_000,
        updated_at: t,
    };
    accounts::upsert(conn, &account).await?;

    let contact_alice = contact(
        &account_id,
        "ct-alice",
        Some("graph-alice"),
        "Alice Chen",
        "alice@example.com",
        t,
    );
    let contact_bob = contact(
        &account_id,
        "ct-bob",
        None,
        "",
        "bob@example.com",
        t,
    );
    let contact_carol = contact(
        &account_id,
        "ct-carol",
        Some("graph-carol"),
        "Carol Wang",
        "carol@example.com",
        t,
    );
    let contact_eve = contact(
        &account_id,
        "ct-eve",
        Some("graph-eve"),
        "Eve Empty",
        "eve@example.com",
        t,
    );
    for c in [&contact_alice, &contact_bob, &contact_carol, &contact_eve] {
        contacts::upsert(conn, c).await?;
    }
    for i in 1..=52 {
        let n = format!("{i:02}");
        contacts::upsert(
            conn,
            &contact(
                &account_id,
                &format!("ct-pad-{n}"),
                Some(&format!("graph-pad-{n}")),
                &format!("Pad Contact {n}"),
                &format!("pad{n}@example.com"),
                t - i * 1000,
            ),
        )
        .await?;
    }

    let group_product = Group {
        id: GroupId::new("grp-product"),
        account_id: account_id.clone(),
        name: "产品评审".into(),
        name_is_manual: true,
        source: GroupSource::Manual,
        send_members_is_manual: true,
        merged_into_id: None,
        created_at: t,
        updated_at: t,
    };
    let group_legacy = Group {
        id: GroupId::new("grp-legacy"),
        account_id: account_id.clone(),
        name: "旧项目组".into(),
        name_is_manual: true,
        source: GroupSource::Auto,
        send_members_is_manual: false,
        merged_into_id: Some(GroupId::new("grp-product")),
        created_at: t - 7_200_000,
        updated_at: t - 3_600_000,
    };
    let group_ops = Group {
        id: GroupId::new("grp-ops"),
        account_id: account_id.clone(),
        name: "运维值班".into(),
        name_is_manual: true,
        source: GroupSource::Manual,
        send_members_is_manual: true,
        merged_into_id: None,
        created_at: t - 86_400_000,
        updated_at: t,
    };
    // 根群组先于 merged 子群组
    groups::upsert(conn, &group_product).await?;
    groups::upsert(conn, &group_ops).await?;
    groups::upsert(conn, &group_legacy).await?;

    groups::upsert_participants(
        conn,
        &[
            participant(&account_id, "grp-product", "alice@example.com", "Alice Chen", true, true, t, t),
            participant(
                &account_id,
                "grp-product",
                "carol@example.com",
                "Carol Wang",
                true,
                false,
                t,
                t,
            ),
            participant(
                &account_id,
                "grp-legacy",
                "bob@example.com",
                "",
                true,
                true,
                t - 7_200_000,
                t - 3_600_000,
            ),
            participant(
                &account_id,
                "grp-ops",
                "carol@example.com",
                "Carol Wang",
                true,
                true,
                t,
                t,
            ),
        ],
    )
    .await?;

    let msg1 = base_msg(
        &account_id,
        "msg-1",
        "g-msg-1",
        Some("thr-alice-1"),
        "下周发布计划",
        "请确认里程碑与风险项……",
        BodyContentType::Text,
        None,
        Some("alice@example.com"),
        "Alice Chen",
        t - 3_600_000,
        false,
        false,
        true,
        Importance::Normal,
        "inbox",
        "收件箱",
        t,
    );
    let msg2 = base_msg(
        &account_id,
        "msg-2",
        "g-msg-2",
        Some("thr-alice-1"),
        "Re: 下周发布计划",
        "我这边没问题，附件里有清单。",
        BodyContentType::Html,
        None,
        Some("me@example.com"),
        "演示用户",
        t - 1_800_000,
        true,
        true,
        false,
        Importance::Normal,
        "sent",
        "已发送",
        t,
    );
    let msg3 = base_msg(
        &account_id,
        "msg-3",
        "g-msg-3",
        Some("thr-group-1"),
        "评审纪要",
        "会议纪要如下……",
        BodyContentType::Text,
        Some("会议纪要如下：\n1. 确认范围\n2. 排期下周".into()),
        Some("carol@example.com"),
        "Carol Wang",
        t - 900_000,
        false,
        false,
        false,
        Importance::High,
        "inbox",
        "收件箱",
        t,
    );
    let msg4 = base_msg(
        &account_id,
        "msg-4",
        "g-msg-4",
        None,
        "无线程占位往来",
        "这是一封没有 ThreadId 的邮件。",
        BodyContentType::Text,
        Some(String::new()),
        Some("bob@example.com"),
        "",
        t - 7_200_000,
        true,
        false,
        false,
        Importance::Low,
        "inbox",
        "收件箱",
        t,
    );
    let msg5 = base_msg(
        &account_id,
        "msg-5",
        "g-msg-5",
        Some("thr-ops-1"),
        "值班交接",
        "今晚告警链路已交接。",
        BodyContentType::Text,
        Some("今晚告警链路已交接，请关注磁盘水位。".into()),
        Some("carol@example.com"),
        "Carol Wang",
        t - 400_000,
        false,
        false,
        false,
        Importance::Normal,
        "inbox",
        "收件箱",
        t,
    );

    put_message(
        conn,
        &msg1,
        &[("me@example.com", "演示用户", RecipientRole::To)],
        Some("请确认里程碑与风险项。\n\n谢谢。"),
    )
    .await?;
    put_message(
        conn,
        &msg2,
        &[("alice@example.com", "Alice Chen", RecipientRole::To)],
        Some(
            "<p>我这边没问题，附件里有清单。</p><p><a href=\"https://example.com\">查看计划</a></p><script>alert(1)</script>",
        ),
    )
    .await?;
    put_message(
        conn,
        &msg3,
        &[
            ("me@example.com", "演示用户", RecipientRole::To),
            ("alice@example.com", "Alice Chen", RecipientRole::Cc),
        ],
        None,
    )
    .await?;
    put_message(
        conn,
        &msg4,
        &[("me@example.com", "演示用户", RecipientRole::To)],
        None,
    )
    .await?;
    put_message(
        conn,
        &msg5,
        &[("me@example.com", "演示用户", RecipientRole::To)],
        None,
    )
    .await?;

    let mut alice_extra_ids = Vec::new();
    for i in 1..=34 {
        let id = format!("msg-alice-hist-{i:02}");
        let at = t - 10_000_000 - i * 60_000;
        let outbound = i % 2 == 0;
        let msg = base_msg(
            &account_id,
            &id,
            &format!("g-{id}"),
            Some("thr-alice-hist"),
            &format!("历史往来 #{i}"),
            &format!("这是 Alice 历史邮件 {i} 的预览。"),
            BodyContentType::Text,
            Some(format!("正文：Alice 历史邮件 {i}")),
            Some(if outbound {
                "me@example.com"
            } else {
                "alice@example.com"
            }),
            if outbound { "演示用户" } else { "Alice Chen" },
            at,
            true,
            outbound,
            false,
            Importance::Normal,
            if outbound { "sent" } else { "inbox" },
            if outbound { "已发送" } else { "收件箱" },
            t,
        );
        let (to_addr, to_name) = if outbound {
            ("alice@example.com", "Alice Chen")
        } else {
            ("me@example.com", "演示用户")
        };
        put_message(conn, &msg, &[(to_addr, to_name, RecipientRole::To)], None).await?;
        alice_extra_ids.push(id);
    }

    let conv_alice = Conversation {
        id: ConversationId::new("conv-alice"),
        account_id: account_id.clone(),
        kind: ConversationKind::Contact,
        normalized_address: Some(EmailAddress::new_unchecked("alice@example.com")),
        contact_id: Some(ContactId::new("ct-alice")),
        group_id: None,
        last_message_at: msg2.received_at,
        last_message_id: Some(MessageId::new("msg-2")),
        unread_count: 1,
        created_at: t,
        updated_at: t,
    };
    let conv_bob = Conversation {
        id: ConversationId::new("conv-bob"),
        account_id: account_id.clone(),
        kind: ConversationKind::Contact,
        normalized_address: Some(EmailAddress::new_unchecked("bob@example.com")),
        contact_id: Some(ContactId::new("ct-bob")),
        group_id: None,
        last_message_at: msg4.received_at,
        last_message_id: Some(MessageId::new("msg-4")),
        unread_count: 0,
        created_at: t,
        updated_at: t,
    };
    let conv_carol = Conversation {
        id: ConversationId::new("conv-carol"),
        account_id: account_id.clone(),
        kind: ConversationKind::Contact,
        normalized_address: Some(EmailAddress::new_unchecked("carol@example.com")),
        contact_id: Some(ContactId::new("ct-carol")),
        group_id: None,
        last_message_at: None,
        last_message_id: None,
        unread_count: 0,
        created_at: t,
        updated_at: t,
    };
    let conv_eve = Conversation {
        id: ConversationId::new("conv-eve"),
        account_id: account_id.clone(),
        kind: ConversationKind::Contact,
        normalized_address: Some(EmailAddress::new_unchecked("eve@example.com")),
        contact_id: Some(ContactId::new("ct-eve")),
        group_id: None,
        last_message_at: None,
        last_message_id: None,
        unread_count: 0,
        created_at: t,
        updated_at: t,
    };
    let conv_product = Conversation {
        id: ConversationId::new("conv-product"),
        account_id: account_id.clone(),
        kind: ConversationKind::Group,
        normalized_address: None,
        contact_id: None,
        group_id: Some(GroupId::new("grp-product")),
        last_message_at: msg3.received_at,
        last_message_id: Some(MessageId::new("msg-3")),
        unread_count: 1,
        created_at: t,
        updated_at: t,
    };
    let conv_ops = Conversation {
        id: ConversationId::new("conv-ops"),
        account_id: account_id.clone(),
        kind: ConversationKind::Group,
        normalized_address: None,
        contact_id: None,
        group_id: Some(GroupId::new("grp-ops")),
        last_message_at: msg5.received_at,
        last_message_id: Some(MessageId::new("msg-5")),
        unread_count: 1,
        created_at: t,
        updated_at: t,
    };

    for c in [
        &conv_alice,
        &conv_bob,
        &conv_carol,
        &conv_eve,
        &conv_product,
        &conv_ops,
    ] {
        conversations::upsert(conn, c).await?;
    }

    let mut alice_links = vec![
        link(&account_id, "conv-alice", "msg-2", t),
        link(&account_id, "conv-alice", "msg-1", t),
    ];
    for id in &alice_extra_ids {
        alice_links.push(link(&account_id, "conv-alice", id, t));
    }
    conversations::attach_messages(conn, &alice_links).await?;
    conversations::attach_messages(conn, &[link(&account_id, "conv-bob", "msg-4", t)])
        .await?;
    conversations::attach_messages(conn, &[link(&account_id, "conv-product", "msg-3", t)])
        .await?;
    conversations::attach_messages(conn, &[link(&account_id, "conv-ops", "msg-5", t)])
        .await?;

    for i in 1..=52 {
        let n = format!("{i:02}");
        let address = format!("pad{n}@example.com");
        let msg_id = format!("msg-pad-{n}");
        let at = t - 20_000_000 - i * 30_000;
        let msg = base_msg(
            &account_id,
            &msg_id,
            &format!("g-{msg_id}"),
            Some(&format!("thr-pad-{n}")),
            &format!("占位会话 {n}"),
            &format!("Pad {n} 预览"),
            BodyContentType::Text,
            Some(format!("Pad {n} 正文")),
            Some(&address),
            &format!("Pad Contact {n}"),
            at,
            i % 7 != 0,
            false,
            false,
            Importance::Normal,
            "inbox",
            "收件箱",
            t,
        );
        put_message(
            conn,
            &msg,
            &[("me@example.com", "演示用户", RecipientRole::To)],
            None,
        )
        .await?;
        let conv = Conversation {
            id: ConversationId::new(format!("conv-pad-{n}")),
            account_id: account_id.clone(),
            kind: ConversationKind::Contact,
            normalized_address: Some(EmailAddress::new_unchecked(address)),
            contact_id: Some(ContactId::new(format!("ct-pad-{n}"))),
            group_id: None,
            last_message_at: Some(at),
            last_message_id: Some(MessageId::new(msg_id.clone())),
            unread_count: if i % 7 == 0 { 1 } else { 0 },
            created_at: at,
            updated_at: at,
        };
        conversations::upsert(conn, &conv).await?;
        conversations::attach_messages(
            conn,
            &[link(&account_id, &format!("conv-pad-{n}"), &msg_id, at)],
        )
        .await?;
    }

    folder_states::upsert(
        conn,
        &MailFolderSyncState {
            account_id: account_id.clone(),
            folder_id: FolderId::new("inbox"),
            delta_link: Some("delta-inbox".into()),
            next_link: None,
            synced_at: Some(t - 60_000),
            sync_status: SyncStatus::Idle,
            last_error: None,
            updated_at: t,
        },
    )
    .await?;
    folder_states::upsert(
        conn,
        &MailFolderSyncState {
            account_id: account_id.clone(),
            folder_id: FolderId::new("archive"),
            delta_link: None,
            next_link: Some("page-archive-2".into()),
            synced_at: None,
            sync_status: SyncStatus::Error,
            last_error: Some("演示：归档文件夹增量失败".into()),
            updated_at: t - 30_000,
        },
    )
    .await?;

    // 与前端一致：最终按附着重算投影，保证 unread / last_message 准确
    for cid in [
        "conv-alice",
        "conv-bob",
        "conv-carol",
        "conv-eve",
        "conv-product",
        "conv-ops",
    ] {
        conversations::recompute_projection(conn, &account_id, &ConversationId::new(cid)).await?;
    }
    for i in 1..=52 {
        let n = format!("{i:02}");
        conversations::recompute_projection(
            conn,
            &account_id,
            &ConversationId::new(format!("conv-pad-{n}")),
        )
        .await?;
    }

    accounts::get(conn, &account_id)
        .await?
        .ok_or_else(|| PortError::Storage {
            message: "演示账户写入后读取失败".into(),
        })
}

fn now_ms() -> TimestampMs {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn contact(
    account_id: &AccountId,
    id: &str,
    graph_id: Option<&str>,
    display_name: &str,
    address: &str,
    at: TimestampMs,
) -> Contact {
    Contact {
        id: ContactId::new(id),
        account_id: account_id.clone(),
        graph_id: graph_id.map(str::to_owned),
        display_name: display_name.into(),
        normalized_address: EmailAddress::new_unchecked(address),
        created_at: at,
        updated_at: at,
    }
}

fn participant(
    account_id: &AccountId,
    group_id: &str,
    address: &str,
    display_name: &str,
    is_observed: bool,
    is_send_member: bool,
    first_seen_at: TimestampMs,
    last_seen_at: TimestampMs,
) -> GroupParticipant {
    GroupParticipant {
        account_id: account_id.clone(),
        group_id: GroupId::new(group_id),
        normalized_address: EmailAddress::new_unchecked(address),
        display_name: display_name.into(),
        is_observed,
        is_send_member,
        first_seen_at,
        last_seen_at,
    }
}

fn link(
    account_id: &AccountId,
    conversation_id: &str,
    message_id: &str,
    attached_at: TimestampMs,
) -> ConversationMessageLink {
    ConversationMessageLink {
        account_id: account_id.clone(),
        conversation_id: ConversationId::new(conversation_id),
        message_id: MessageId::new(message_id),
        attached_at,
    }
}

#[allow(clippy::too_many_arguments)]
fn base_msg(
    account_id: &AccountId,
    id: &str,
    graph_id: &str,
    thread_id: Option<&str>,
    subject: &str,
    body_preview: &str,
    body_content_type: BodyContentType,
    body_content: Option<String>,
    from_address: Option<&str>,
    from_name: &str,
    at: TimestampMs,
    is_read: bool,
    is_outbound: bool,
    has_attachments: bool,
    importance: Importance,
    folder_id: &str,
    parent_folder_name: &str,
    local_updated_at: TimestampMs,
) -> Message {
    Message {
        id: MessageId::new(id),
        account_id: account_id.clone(),
        graph_id: Some(GraphImmutableId::new(graph_id)),
        thread_id: thread_id.map(ThreadId::new),
        graph_change_key: None,
        internet_message_id: None,
        conversation_index: None,
        subject: subject.into(),
        body_preview: body_preview.into(),
        body_content_type,
        body_content,
        from_address: from_address.map(EmailAddress::new_unchecked),
        from_name: from_name.into(),
        received_at: Some(at),
        sent_at: Some(at),
        created_at_graph: Some(at),
        is_read,
        is_draft: false,
        is_outbound,
        has_attachments,
        importance,
        folder_id: Some(FolderId::new(folder_id)),
        parent_folder_name: Some(parent_folder_name.into()),
        sync_status: MessageSyncStatus::Synced,
        local_updated_at,
        remote_updated_at: Some(local_updated_at),
    }
}

async fn put_message(
    conn: &mut SqliteConnection,
    message: &Message,
    recipients: &[(&str, &str, RecipientRole)],
    pending_body: Option<&str>,
) -> Result<(), PortError> {
    let mut msg = message.clone();
    if let Some(body) = pending_body {
        // 种子里 pending 表示「可按需加载」：库内先存为未加载（null），ensure 时写入
        // 为便于本阶段本地 ensure_message_body，直接落已加载正文；与前端 pendingBodies 演示等价呈现
        msg.body_content = Some(body.to_owned());
    }
    let recipients = recipients
        .iter()
        .enumerate()
        .map(|(seq, (addr, name, role))| MessageRecipient {
            account_id: msg.account_id.clone(),
            message_id: msg.id.clone(),
            role: *role,
            seq: seq as i32,
            normalized_address: EmailAddress::new_unchecked(*addr),
            display_name: (*name).into(),
        })
        .collect();
    messages::upsert_with_recipients(
        conn,
        &MessageWithRecipients {
            message: msg,
            recipients,
        },
    )
    .await
}
