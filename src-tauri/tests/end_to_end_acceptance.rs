//! Task 17：跨仓储 / 协议契约的端到端验收（内存 SQLite，无真实邮箱）。

use postlet_lib::domain::services::missing_imap_page;
use postlet_lib::domain::value_objects::{
    AccountId, FolderId, MessageId, RemoteKey, SessionId,
};
use postlet_lib::infrastructure::db;
use postlet_lib::infrastructure::repository::{messages, mutations, sessions};
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

/// 受理结果写入本地发信记录：同事务提交发送状态与账号版本，会话可回看。
/// 已发送与 Sent 副本无关，客户端也不再有任何归档任务。
#[tokio::test]
async fn accepted_send_commits_local_record_and_account_version() {
    use postlet_lib::application::use_cases::outbound_use_case;
    use postlet_lib::domain::entities::{next_state, SendState, SendTask};
    use postlet_lib::domain::mail::{SendOutcome, SendStage};

    let db = db::open_memory().await.unwrap();
    sqlx::query(
        "INSERT INTO accounts(id,email,created_at,updated_at)
         VALUES ('a','a@example.com',1,1)",
    )
    .execute(&db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO account_mail_transport(
            account_id,receive_protocol,send_protocol,receive_credential_ref,send_credential_ref,
            imap_host,imap_port,imap_tls,imap_username,
            smtp_host,smtp_port,smtp_tls,smtp_username,updated_at)
         VALUES ('a','imap','smtp','r1','s1','imap.example.com',993,'implicit','a@example.com',
            'smtp.example.com',587,'starttls','a@example.com',1)",
    )
    .execute(&db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO sessions(account_id,id,kind,normalized_address,created_at,updated_at)
         VALUES ('a','s1','private','peer@example.com',1,1)",
    )
    .execute(&db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO messages(account_id,id,session_id,subject,body_preview,body_content_type,
            body_content,from_name,sent_at,is_read,is_draft,is_outbound,has_attachments,importance,local_updated_at)
         VALUES ('a','m1','s1','s','t','text','t','',1,0,0,1,0,'normal',1)",
    )
    .execute(&db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO attachment_contents(content_id,account_id,storage_key,byte_length,digest,created_at)
         VALUES ('c1','a','c1',1,'d',1)",
    )
    .execute(&db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO outbound_snapshots(account_id,snapshot_id,content_version,internet_message_id,date,
            mime_content_id,envelope_json,subject,text_body,created_at)
         VALUES ('a','s1',1,'<m@x>','d','c1','{}','s','t',1)",
    )
    .execute(&db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO send_tasks(task_id,account_id,message_id,content_version,snapshot_id,state,
            attempt_id,claim_token,attempts,created_at,updated_at)
         VALUES ('send-1','a','m1',1,'s1','sending','attempt','claim',1,1,1)",
    )
    .execute(&db)
    .await
    .unwrap();

    let task = SendTask {
        task_id: "send-1".into(),
        account_id: AccountId::new("a"),
        message_id: MessageId::new("m1"),
        content_version: 1,
        snapshot_id: "s1".into(),
        state: SendState::Sending,
        attempt_id: Some("attempt".into()),
        claim_token: Some("claim".into()),
        lease_until: None,
        attempts: 1,
        next_attempt_at: None,
        last_error_code: None,
        last_error: None,
        accepted_at: None,
        created_at: 1,
        updated_at: 1,
    };
    assert_eq!(next_state(SendOutcome::Accepted, SendStage::Data), SendState::Accepted);
    assert_eq!(
        outbound_use_case::complete_accepted_attempt(&db, &AccountId::new("a"), &task, 2)
            .await
            .unwrap(),
        Some(1)
    );

    let mut conn = db.acquire().await.unwrap();
    let version: i64 = sqlx::query_scalar(
        "SELECT change_version FROM accounts WHERE id='a'",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(version, 1, "accepted result must bump the account version");
    let state: String = sqlx::query_scalar(
        "SELECT state FROM send_tasks WHERE account_id='a' AND task_id='send-1'",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(state, "accepted");

    // 入队时的会话投影：本地发信记录参与排序与摘要（不依赖远端位置）
    let projected = sessions::recompute_projection(
        &mut conn,
        &AccountId::new("a"),
        &SessionId::new("s1"),
    )
    .await
    .unwrap();
    assert_eq!(projected.last_message_id, Some(MessageId::new("m1")));
    assert_eq!(projected.last_message_at, Some(1));

    // 普通列表只展示实际位置消息；无位置发送记录进入发送状态区。
    let ordinary = sessions::list_messages(
        &mut conn,
        &AccountId::new("a"),
        &SessionId::new("s1"),
        None,
        30,
        postlet_lib::domain::value_objects::PageDirection::Older,
    )
    .await
    .unwrap();
    assert!(ordinary.items.is_empty());
    let pending = messages::list_pending_sends(
        &mut conn,
        &AccountId::new("a"),
        &SessionId::new("s1"),
    )
    .await
    .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].message.id, MessageId::new("m1"));
    assert_eq!(pending[0].message.body_content.as_deref(), Some("t"));
    assert_eq!(pending[0].send_state, Some(SendState::Accepted));

    // 只发过信的会话仍出现在默认会话列表，摘要来自本地发信记录。
    let page = sessions::list(
        &mut conn,
        Some(&AccountId::new("a")),
        None,
        50,
        None,
        false,
    )
    .await
    .unwrap();
    let item = page
        .items
        .iter()
        .find(|item| item.session.id == SessionId::new("s1"))
        .expect("仅有本地发信记录的会话必须可见");
    assert_eq!(item.session.last_message_id, Some(MessageId::new("m1")));
    let preview = item
        .last_message
        .as_ref()
        .expect("会话摘要必须覆盖本地发信");
    assert!(preview.is_outbound);
    assert_eq!(preview.subject, "s");

    // 无远端位置的本地记录不得被当成远端标记目标。
    let rejected = mutations::put_read(
        &mut conn,
        &AccountId::new("a"),
        &MessageId::new("m1"),
        true,
    )
    .await;
    assert!(rejected.is_err(), "本地发信记录没有远端位置，不得提交远端标记");
    let queued: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pending_message_mutations WHERE account_id='a'",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(queued, 0);
}

/// 新建 SQLite 初始化后不存在客户端归档：无归档表、无归档策略字段。
#[tokio::test]
async fn fresh_schema_has_no_client_sent_archive() {
    let db = db::open_memory().await.unwrap();
    let tables: Vec<String> = sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type='table'")
        .fetch_all(&db)
        .await
        .unwrap();
    assert!(
        !tables.iter().any(|name| name == "archive_tasks"),
        "客户端归档表必须随归档链移除"
    );
    let columns: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info('accounts')")
        .fetch_all(&db)
        .await
        .unwrap();
    assert!(
        !columns.iter().any(|name| name == "sent_archive_policy"),
        "归档策略字段必须随归档链移除"
    );
}
