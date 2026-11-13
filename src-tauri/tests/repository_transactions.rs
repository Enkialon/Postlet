//! 用真实内存 SQLite（migrate）验证模块调用、账户隔离及跨仓储事务。

use postlet_lib::application::use_cases::{account_use_case, contact_use_case, conversation_use_case};
use postlet_lib::application::dto::{ConversationListQuery, ConversationMessagesQuery};
use postlet_lib::domain::entities::{Contact, Group};
use postlet_lib::domain::value_objects::{
    AccountId, ContactId, ConversationId, EmailAddress, GroupId, GroupSource, PageDirection,
};
use postlet_lib::infrastructure::db;
use postlet_lib::infrastructure::repository::{contacts, groups};
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
        merged_into_id: None,
        created_at: 1,
        updated_at: 1,
    }
}

#[tokio::test]
async fn application_calls_repository_and_preserves_account_isolation() {
    let db = database().await;
    // 账户外键：先插入账户
    sqlx::query(
        "INSERT INTO accounts (id, email, display_name, aliases_json, mail_sync_status, show_group_messages_in_contact_conversation, created_at, updated_at)
         VALUES ('a', 'a@example.com', 'a', '[]', 'idle', 0, 1, 1),
                ('b', 'b@example.com', 'b', '[]', 'idle', 0, 1, 1)",
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
        "INSERT INTO accounts (id, email, display_name, aliases_json, mail_sync_status, show_group_messages_in_contact_conversation, created_at, updated_at)
         VALUES ('a', 'a@example.com', 'a', '[]', 'idle', 0, 1, 1)",
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
        "INSERT INTO accounts (id, email, display_name, aliases_json, mail_sync_status, show_group_messages_in_contact_conversation, created_at, updated_at)
         VALUES ('a', 'a@example.com', 'a', '[]', 'idle', 0, 1, 1)",
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

#[tokio::test]
async fn load_demo_workspace_supports_conversation_reads() {
    let db = database().await;
    let account = account_use_case::load_demo_workspace(&db).await.unwrap();
    assert_eq!(account.id.as_str(), "acc-demo");

    let page = conversation_use_case::list_conversations(
        &db,
        ConversationListQuery {
            account_id: account.id.clone(),
            cursor: None,
            limit: 50,
            kind: None,
        },
    )
    .await
    .unwrap();
    assert!(page.items.len() >= 50);
    assert!(page.has_more);

    let alice = conversation_use_case::get_conversation(
        &db,
        &account.id,
        &ConversationId::new("conv-alice"),
    )
    .await
    .unwrap();
    assert_eq!(
        alice.contact_display_name.as_deref(),
        Some("Alice Chen")
    );
    assert!(alice.last_message_preview.is_some());

    let messages = conversation_use_case::list_messages(
        &db,
        ConversationMessagesQuery {
            account_id: account.id.clone(),
            conversation_id: ConversationId::new("conv-alice"),
            cursor: None,
            limit: 30,
            direction: PageDirection::Older,
        },
    )
    .await
    .unwrap();
    assert_eq!(messages.items.len(), 30);
    assert!(messages.has_more);
}
