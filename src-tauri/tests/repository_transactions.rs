//! 用真实内存 SQLite 验证模块调用、账户隔离及跨仓储事务。

use postlet_lib::application::use_cases::contact_use_case;
use postlet_lib::domain::entities::{Contact, Group};
use postlet_lib::domain::value_objects::{
    AccountId, ContactId, EmailAddress, GroupId, GroupSource,
};
use postlet_lib::infrastructure::repository::{contacts, groups};
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};

async fn database() -> SqlitePool {
    let db = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql("CREATE TABLE contacts (id TEXT NOT NULL, account_id TEXT NOT NULL, graph_id TEXT, display_name TEXT NOT NULL, normalized_address TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, UNIQUE(account_id, id), UNIQUE(account_id, normalized_address));
        CREATE TABLE groups (id TEXT NOT NULL, account_id TEXT NOT NULL, name TEXT NOT NULL, name_is_manual INTEGER NOT NULL, source TEXT NOT NULL, send_members_is_manual INTEGER NOT NULL, merged_into_id TEXT, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, UNIQUE(account_id, id));")
        .execute(&db).await.unwrap();
    db
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
    let mut conn = db.acquire().await.unwrap();
    groups::insert(&mut conn, &group()).await.unwrap();
    drop(conn);
    let mut tx = db.begin().await.unwrap();
    contacts::upsert(&mut tx, &contact("a")).await.unwrap();
    assert!(groups::insert(&mut tx, &group()).await.is_err());
    // Application 遇错必须返回或显式回滚，Repository 不得自行提交。
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
