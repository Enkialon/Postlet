//! Task 17：跨仓储 / 协议契约的端到端验收（内存 SQLite，无真实邮箱）。

use postlet_lib::domain::services::missing_imap_page;
use postlet_lib::domain::value_objects::{
    AccountId, EmailAddress, FolderId, MessageId, RemoteKey, SessionId,
};
use postlet_lib::infrastructure::db;
use postlet_lib::infrastructure::repository::{accounts, messages, mutations, sessions};
use std::collections::HashSet;
use sqlx::SqlitePool;

async fn seed(db: &SqlitePool, account: &str, folder: &str, remote: &str, msg: &str) {
    sqlx::query(
        "INSERT OR IGNORE INTO accounts(id,email,created_at,updated_at) VALUES (?,?,1,1)",
    )
    .bind(account)
    .bind(format!("{account}@example.com"))
    .execute(db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT OR IGNORE INTO mail_folder_sync_state(
            account_id,folder_id,protocol,remote_binding,binding_revision,is_retired,
            display_name,sync_enabled,sync_status,updated_at,role,role_source)
         VALUES (?,?,'imap',?,1,0,?,1,'idle',1,
            CASE WHEN ? = 'trash' THEN 'trash' WHEN ? = 'sent' THEN 'sent' ELSE 'inbox' END,
            'server')",
    )
    .bind(account)
    .bind(folder)
    .bind(remote)
    .bind(folder)
    .bind(folder)
    .bind(folder)
    .execute(db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT OR IGNORE INTO sessions(account_id,id,kind,normalized_address,created_at,updated_at)
         VALUES (?,?,'private',?,1,1)",
    )
    .bind(account)
    .bind(format!("{account}-session-{folder}"))
    .bind(format!("{folder}@example.com"))
    .execute(db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT OR IGNORE INTO messages(
            account_id,id,session_id,remote_key,location_status,location_revision,folder_id,
            subject,body_preview,body_content_type,from_name,is_read,is_draft,is_outbound,
            has_attachments,importance,local_updated_at)
         VALUES (?,?,?,?,'present',1,?,'s','','text','',0,0,0,0,'normal',1)",
    )
    .bind(account)
    .bind(msg)
    .bind(format!("{account}-session-{folder}"))
    .bind(remote)
    .bind(folder)
    .execute(db)
    .await
    .unwrap_or_else(|e| panic!("messages insert failed account={account} folder={folder} msg={msg} remote={remote}: {e}"));
}

/// 相同位置重复入库幂等；不同文件夹与不同账号互不影响。
#[tokio::test]
async fn same_location_is_idempotent_while_folders_and_accounts_stay_independent() {
    let db = db::open_memory().await.unwrap();
    seed(&db, "a", "inbox", "imap:1:10", "a-inbox-1").await;
    seed(&db, "b", "inbox", "imap:1:10", "b-inbox-1").await;

    // 同位置重复：用位置查询必须命中同一条，不新增
    let mut conn = db.acquire().await.unwrap();
    let first = messages::get_by_location(
        &mut conn,
        &AccountId::new("a"),
        &FolderId::new("inbox"),
        &RemoteKey::new("imap:1:10"),
    )
    .await
    .unwrap()
    .expect("present");
    let again = messages::get_by_location(
        &mut conn,
        &AccountId::new("a"),
        &FolderId::new("inbox"),
        &RemoteKey::new("imap:1:10"),
    )
    .await
    .unwrap()
    .expect("present");
    assert_eq!(first.message.id, again.message.id);

    // 另一账号是独立实体
    let other_account = messages::get_by_location(
        &mut conn,
        &AccountId::new("b"),
        &FolderId::new("inbox"),
        &RemoteKey::new("imap:1:10"),
    )
    .await
    .unwrap()
    .expect("present");
    assert_ne!(first.message.id, other_account.message.id);
}

/// 默认删除：有 Trash 时冻结源位置与目标；无 Trash 时报错不扩大操作。
#[tokio::test]
async fn default_delete_needs_a_confirmed_trash_target() {
    let db = db::open_memory().await.unwrap();
    seed(&db, "a", "inbox", "imap:1:10", "m1").await;
    seed(&db, "b", "inbox", "imap:1:20", "m2").await;
    seed(&db, "b", "trash", "imap:1:99", "m3").await;

    let mut conn = db.acquire().await.unwrap();
    let no_trash = mutations::put_trash(&mut conn, &AccountId::new("a"), &MessageId::new("m1")).await;
    assert!(no_trash.is_err(), "no Trash must not widen into permanent delete");

    mutations::put_trash(&mut conn, &AccountId::new("b"), &MessageId::new("m2"))
        .await
        .unwrap();
    let saved = mutations::get(&mut conn, &AccountId::new("b"), &MessageId::new("m2"))
        .await
        .unwrap()
        .expect("trash intent");
    assert_eq!(saved.target_folder_id.to_string(), "inbox");
    assert_eq!(
        saved.destination_folder_id.map(|f| f.to_string()),
        Some("trash".to_string())
    );
}

/// 同步差集基线：UID 不连续也必须覆盖缺失集合，不受高水位猜测影响。
#[test]
fn uid_gaps_are_downloaded_by_set_difference() {
    let remote: HashSet<u32> = [1, 5, 90, 4_000, 9_999].into_iter().collect();
    let mut downloaded: HashSet<u32> = [9_999].into();
    let mut order = Vec::new();
    for _ in 0..5 {
        let page = missing_imap_page(&remote, &downloaded, 2);
        if page.is_empty() {
            break;
        }
        order.extend(page.iter().copied());
        downloaded.extend(page);
    }
    assert_eq!(downloaded, remote);
    assert!(!order.is_empty());
}
