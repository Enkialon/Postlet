//! 持久化回填进度驱动后台续跑：完成后才允许进入 IDLE。

use postlet_lib::application::use_cases::sync_use_case::has_pending_imap_history;
use postlet_lib::domain::entities::MailFolderSyncState;
use postlet_lib::domain::value_objects::{AccountId, FolderId, SyncStatus};
use postlet_lib::infrastructure::repository::folder_states;
use postlet_lib::infrastructure::{
    db,
    mail::cursor::{encode_uid_cursor, encode_uidvalidity},
};

#[tokio::test]
async fn only_enabled_unfinished_folders_keep_history_running_after_commit() {
    let db = db::open_memory().await.unwrap();
    sqlx::query("INSERT INTO accounts(id,email,created_at,updated_at) VALUES ('a','a@example.com',1,1),('b','b@example.com',1,1)")
        .execute(&db).await.unwrap();
    let a = AccountId::new("a");
    let b = AccountId::new("b");
    let mut folder = MailFolderSyncState {
        account_id: a.clone(),
        folder_id: FolderId::new("local-inbox"),
        protocol: postlet_lib::domain::value_objects::ReceiveProtocol::Imap,
        remote_binding: "INBOX".into(), binding_revision: 1, is_retired: false,
        parent_folder_id: None,
        display_name: "INBOX".into(),
        sync_enabled: true,
        generation: None,
        sync_cursor: None,
        page_cursor: None,
        synced_at: None,
        sync_status: SyncStatus::Idle,
        last_error: None,
        updated_at: 1,
    };
    async fn save(db: &sqlx::SqlitePool, folder: &MailFolderSyncState) {
        let mut tx = db.begin().await.unwrap();
        folder_states::upsert(&mut tx, folder).await.unwrap();
        tx.commit().await.unwrap();
    }
    save(&db, &folder).await;
    assert!(has_pending_imap_history(&db, &a).await.unwrap());
    assert!(!has_pending_imap_history(&db, &b).await.unwrap());

    folder.sync_cursor = Some(encode_uidvalidity(7));
    folder.page_cursor = Some(encode_uid_cursor(1000, 999));
    save(&db, &folder).await;
    assert!(has_pending_imap_history(&db, &a).await.unwrap());

    folder.page_cursor = Some(encode_uid_cursor(1000, 1));
    save(&db, &folder).await;
    assert!(!has_pending_imap_history(&db, &a).await.unwrap());

    folder.folder_id = FolderId::new("local-archive");
    folder.remote_binding = "Archive".into();
    folder.page_cursor = None;
    save(&db, &folder).await;
    assert!(has_pending_imap_history(&db, &a).await.unwrap());
    folder.sync_enabled = false;
    save(&db, &folder).await;
    assert!(!has_pending_imap_history(&db, &a).await.unwrap());
}
