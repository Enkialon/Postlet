//! 用真实内存 SQLite（migrate）验证模块调用、账户隔离及跨仓储事务。

use postlet_lib::application::use_cases::{contact_use_case, session_use_case};
use postlet_lib::application::dto::{ContactListQuery, GroupListQuery, SessionListQuery, SessionMessagesQuery};
use postlet_lib::domain::entities::{
    Account, Contact, Group, Message, MessageWithRecipients, Session,
};
use postlet_lib::domain::value_objects::{
    AccountId, BodyContentType, ContactId, ContactSource, EmailAddress, FolderId, GroupId,
    GroupSource, Importance, MessageId, MessageSyncStatus, PageDirection, SessionId, SessionKind,
    SyncStatus,
};
use postlet_lib::infrastructure::db;
use postlet_lib::infrastructure::repository::{
    accounts, contacts, groups, messages, sessions,
};
use sqlx::SqlitePool;

async fn database() -> SqlitePool {
    db::open_memory().await.unwrap()
}

fn contact(account: &str) -> Contact {
    Contact {
        id: ContactId::new("contact"),
        account_id: AccountId::new(account),
        graph_id: None,
        display_name: account.into(),
        normalized_address: EmailAddress::parse("peer@example.com").unwrap(),
        source: ContactSource::Manual,
        created_at: 1,
        updated_at: 1,
    }
}

fn group() -> Group {
    Group {
        id: GroupId::new("group"),
        account_id: AccountId::new("a"),
        name: "项目组".into(),
        name_is_manual: true,
        source: GroupSource::Manual,
        send_members_is_manual: false,
        created_at: 1,
        updated_at: 1,
    }
}

#[tokio::test]
async fn application_calls_repository_and_preserves_account_isolation() {
    let db = database().await;
    sqlx::query(
        "INSERT INTO accounts (id, email, display_name, aliases_json, mail_sync_status, created_at, updated_at)
         VALUES ('a', 'a@example.com', 'a', '[]', 'idle', 1, 1),
                ('b', 'b@example.com', 'b', '[]', 'idle', 1, 1)",
    )
    .execute(&db)
    .await
    .unwrap();
    let mut conn = db.acquire().await.unwrap();
    contacts::upsert(&mut conn, &contact("a")).await.unwrap();
    contacts::upsert(&mut conn, &contact("b")).await.unwrap();
    drop(conn);
    let found =
        contact_use_case::get_contact(&db, &AccountId::new("b"), &ContactId::new("contact"))
            .await
            .unwrap();
    assert_eq!(found.display_name, "b");
    let missing =
        contact_use_case::get_contact(&db, &AccountId::new("c"), &ContactId::new("contact"))
            .await
            .unwrap_err();
    assert_eq!(
        missing.code,
        postlet_lib::application::AppErrorCode::NotFound
    );
}

#[tokio::test]
async fn different_repositories_share_commit_and_rollback() {
    let db = database().await;
    sqlx::query(
        "INSERT INTO accounts (id, email, display_name, aliases_json, mail_sync_status, created_at, updated_at)
         VALUES ('a', 'a@example.com', 'a', '[]', 'idle', 1, 1)",
    )
    .execute(&db)
    .await
    .unwrap();
    let mut tx = db.begin().await.unwrap();
    contacts::upsert(&mut tx, &contact("a")).await.unwrap();
    groups::insert(&mut tx, &group()).await.unwrap();
    tx.rollback().await.unwrap();
    let mut conn = db.acquire().await.unwrap();
    assert!(
        contacts::get(&mut conn, &AccountId::new("a"), &ContactId::new("contact"))
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        groups::get(&mut conn, &AccountId::new("a"), &GroupId::new("group"))
            .await
            .unwrap()
            .is_none()
    );
    drop(conn);
    let mut tx = db.begin().await.unwrap();
    contacts::upsert(&mut tx, &contact("a")).await.unwrap();
    groups::insert(&mut tx, &group()).await.unwrap();
    tx.commit().await.unwrap();
    let mut conn = db.acquire().await.unwrap();
    assert!(
        contacts::get(&mut conn, &AccountId::new("a"), &ContactId::new("contact"))
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        groups::get(&mut conn, &AccountId::new("a"), &GroupId::new("group"))
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn failure_in_second_repository_rolls_back_first_write() {
    let db = database().await;
    sqlx::query(
        "INSERT INTO accounts (id, email, display_name, aliases_json, mail_sync_status, created_at, updated_at)
         VALUES ('a', 'a@example.com', 'a', '[]', 'idle', 1, 1)",
    )
    .execute(&db)
    .await
    .unwrap();
    let mut conn = db.acquire().await.unwrap();
    groups::insert(&mut conn, &group()).await.unwrap();
    drop(conn);
    let mut tx = db.begin().await.unwrap();
    contacts::upsert(&mut tx, &contact("a")).await.unwrap();
    assert!(groups::insert(&mut tx, &group()).await.is_err());
    drop(tx);
    let mut conn = db.acquire().await.unwrap();
    assert!(
        contacts::get(&mut conn, &AccountId::new("a"), &ContactId::new("contact"))
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        groups::get(&mut conn, &AccountId::new("a"), &GroupId::new("group"))
            .await
            .unwrap()
            .is_some()
    );
}

async fn seed_signed_in_mailbox(db: &SqlitePool) -> AccountId {
    let mut conn = db.acquire().await.unwrap();
    let account_id = AccountId::new("acc-test");
    let peer = EmailAddress::parse("alice@example.com").unwrap();
    let contact_id = ContactId::new("contact-alice");
    let session_id = SessionId::new("sess-alice");
    let msg_id = MessageId::new("msg-1");

    accounts::upsert(
        &mut conn,
        &Account {
            id: account_id.clone(),
            email: EmailAddress::parse("me@example.com").unwrap(),
            display_name: "测试用户".into(),
            aliases: vec![],
            tenant_id: None,
            home_account_id: None,
            contact_delta_link: None,
            mail_sync_triggered_at: Some(1_000),
            contact_synced_at: None,
            mail_sync_status: SyncStatus::Idle,
            mail_sync_error: None,
            include_in_all: true,
            created_at: 1,
            updated_at: 1,
        },
    )
    .await
    .unwrap();

    contacts::upsert(
        &mut conn,
        &Contact {
            id: contact_id.clone(),
            account_id: account_id.clone(),
            graph_id: None,
            display_name: "Alice Chen".into(),
            normalized_address: peer.clone(),
            source: ContactSource::Manual,
            created_at: 1,
            updated_at: 1,
        },
    )
    .await
    .unwrap();

    sessions::upsert(
        &mut conn,
        &Session {
            id: session_id.clone(),
            account_id: account_id.clone(),
            kind: SessionKind::Private,
            normalized_address: Some(peer.clone()),
            contact_id: Some(contact_id),
            group_id: None,
            merged_into_id: None,
            last_message_at: Some(2_000),
            last_message_id: Some(msg_id.clone()),
            unread_count: 1,
            hidden_at: None,
            created_at: 1,
            updated_at: 2_000,
        },
    )
    .await
    .unwrap();

    messages::upsert_with_recipients(
        &mut conn,
        &MessageWithRecipients {
            message: Message {
                id: msg_id.clone(),
                account_id: account_id.clone(),
                session_id: session_id.clone(),
                graph_id: None,
                graph_change_key: None,
                internet_message_id: None,
                in_reply_to: None,
                subject: "你好".into(),
                body_preview: "正文预览".into(),
                body_content_type: BodyContentType::Text,
                body_content: Some("正文".into()),
                from_address: Some(peer),
                from_name: "Alice Chen".into(),
                received_at: Some(2_000),
                sent_at: None,
                created_at_graph: None,
                is_read: false,
                is_draft: false,
                is_outbound: false,
                has_attachments: false,
                importance: Importance::Normal,
                folder_id: Some(FolderId::new("inbox")),
                parent_folder_name: Some("Inbox".into()),
                sync_status: MessageSyncStatus::Synced,
                local_updated_at: 2_000,
                remote_updated_at: Some(2_000),
            },
            recipients: vec![],
        },
    )
    .await
    .unwrap();

    account_id
}

#[tokio::test]
async fn seeded_mailbox_supports_session_reads() {
    let db = database().await;
    let account_id = seed_signed_in_mailbox(&db).await;

    let page = session_use_case::list_sessions(
        &db,
        SessionListQuery {
            account_id: Some(account_id.clone()),
            cursor: None,
            limit: 20,
            kind: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(page.items.len(), 1);
    assert!(!page.has_more);

    let alice = session_use_case::get_session(
        &db,
        &account_id,
        &SessionId::new("sess-alice"),
    )
    .await
    .unwrap();
    assert_eq!(alice.contact_display_name.as_deref(), Some("Alice Chen"));
    assert_eq!(alice.display_name, "Alice Chen");
    assert!(alice.last_message_preview.is_some());

    let msgs = session_use_case::list_messages(
        &db,
        SessionMessagesQuery {
            account_id: account_id.clone(),
            session_id: SessionId::new("sess-alice"),
            cursor: None,
            limit: 30,
            direction: PageDirection::Older,
        },
    )
    .await
    .unwrap();
    assert_eq!(msgs.items.len(), 1);
    assert!(!msgs.has_more);
}

#[tokio::test]
async fn all_account_lists_hide_data_while_signed_out_keeps_account_row() {
    let db = database().await;
    let account_id = seed_signed_in_mailbox(&db).await;
    let mut conn = db.acquire().await.unwrap();

    let before = sessions::list(
        &mut conn,
        &SessionListQuery {
            account_id: None,
            cursor: None,
            limit: 20,
            kind: None,
        },
    )
    .await
    .unwrap();
    assert!(!before.items.is_empty());

    accounts::set_signed_in(&mut conn, &account_id, false)
        .await
        .unwrap();
    assert!(accounts::list_ids(&mut conn).await.unwrap().is_empty());
    assert!(
        accounts::get(&mut conn, &account_id)
            .await
            .unwrap()
            .is_some(),
        "退出后账户行仍在，便于再登录关联"
    );

    let sess = sessions::list(
        &mut conn,
        &SessionListQuery {
            account_id: None,
            cursor: None,
            limit: 20,
            kind: None,
        },
    )
    .await
    .unwrap();
    assert!(sess.items.is_empty());

    let cts = contacts::list(
        &mut conn,
        &ContactListQuery {
            account_id: None,
            cursor: None,
            limit: 20,
            search: None,
        },
    )
    .await
    .unwrap();
    assert!(cts.items.is_empty());

    let gs = groups::list(
        &mut conn,
        &GroupListQuery {
            account_id: None,
            cursor: None,
            limit: 20,
            search: None,
        },
    )
    .await
    .unwrap();
    assert!(gs.items.is_empty());

    accounts::set_signed_in(&mut conn, &account_id, true)
        .await
        .unwrap();
    let again = sessions::list(
        &mut conn,
        &SessionListQuery {
            account_id: None,
            cursor: None,
            limit: 20,
            kind: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(again.items.len(), before.items.len());

    // 已登录但关掉 include_in_all：不进「全部」，单账号过滤仍可见
    accounts::update_display_settings(&mut conn, &account_id, false)
        .await
        .unwrap();
    assert!(accounts::list_ids(&mut conn).await.unwrap().is_empty());
    let hidden_in_all = sessions::list(
        &mut conn,
        &SessionListQuery {
            account_id: None,
            cursor: None,
            limit: 20,
            kind: None,
        },
    )
    .await
    .unwrap();
    assert!(hidden_in_all.items.is_empty());
    let single = sessions::list(
        &mut conn,
        &SessionListQuery {
            account_id: Some(account_id.clone()),
            cursor: None,
            limit: 20,
            kind: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(single.items.len(), before.items.len());
}
