//! Task 03：受管理附件与 SQLite 草稿 — 共享引用、重启持久化、缺失内容报错。

use base64::Engine;
use postlet_lib::application::dto::{
    ComposeAttachmentDraft, ImportAttachmentCommand, SaveComposeDraftCommand,
};
use postlet_lib::application::use_cases::{attachment_use_case, compose_use_case};
use postlet_lib::application::AppErrorCode;
use postlet_lib::domain::entities::{Message, MessageWithRecipients};
use postlet_lib::domain::value_objects::{
    AccountId, BodyContentType, Importance, MessageId, SessionId, SessionKind,
};
use postlet_lib::infrastructure::attachment_store;
use postlet_lib::infrastructure::db;
use postlet_lib::infrastructure::paths::AppPaths;
use postlet_lib::infrastructure::repository::{attachments, messages, sessions};
use postlet_lib::domain::entities::Session;
use sqlx::SqlitePool;
use std::path::Path;

fn paths(root: &Path) -> AppPaths {
    let p = AppPaths {
        config_dir: root.join("config"),
        local_data_dir: root.to_owned(),
        data_dir: root.join("data"),
        attachments_dir: root.join("attachments"),
        credentials_dir: root.join("credentials"),
        database_path: root.join("data/postlet.db"),
    };
    for dir in [
        &p.config_dir,
        &p.local_data_dir,
        &p.data_dir,
        &p.attachments_dir,
        &p.credentials_dir,
    ] {
        std::fs::create_dir_all(dir).unwrap();
    }
    p
}

async fn seed_account_session(db: &SqlitePool, account: &str, session: &str) {
    sqlx::query(
        "INSERT INTO accounts (id, email, display_name, aliases_json, mail_sync_status, created_at, updated_at)
         VALUES (?, ?, 'Test', '[]', 'idle', 1, 1)",
    )
    .bind(account)
    .bind(format!("{account}@example.com"))
    .execute(db)
    .await
    .unwrap();

    let mut conn = db.acquire().await.unwrap();
    sessions::upsert(
        &mut conn,
        &Session {
            id: SessionId::new(session),
            account_id: AccountId::new(account),
            kind: SessionKind::Private,
            normalized_address: Some(
                postlet_lib::domain::value_objects::EmailAddress::parse("peer@example.com")
                    .unwrap(),
            ),
            contact_id: None,
            group_id: None,
            merged_into_id: None,
            last_message_at: None,
            last_message_id: None,
            unread_count: 0,
            hidden_at: None,
            created_at: 1,
            updated_at: 1,
        },
    )
    .await
    .unwrap();
}

fn import_cmd(
    account: &str,
    session: &str,
    name: &str,
    bytes: &[u8],
) -> ImportAttachmentCommand {
    ImportAttachmentCommand {
        account_id: AccountId::new(account),
        session_id: SessionId::new(session),
        name: name.into(),
        mime: "application/octet-stream".into(),
        byte_length: bytes.len() as i64,
        content_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
    }
}

#[tokio::test]
async fn shared_content_ref_survives_when_one_owner_deleted() {
    let root = tempfile::tempdir().unwrap();
    let paths = paths(root.path());
    let db = db::open_memory().await.unwrap();
    seed_account_session(&db, "a", "s1").await;

    let att = attachment_use_case::import_attachment(
        &db,
        &paths,
        import_cmd("a", "s1", "shared.bin", b"\0\x01\x02payload"),
    )
    .await
    .unwrap();

    let account = AccountId::new("a");
    let message_id = MessageId::new("msg-shared");
    let mut tx = db.begin().await.unwrap();
    messages::upsert_with_recipients(
        &mut tx,
        &MessageWithRecipients {
            send_state: None,
            pending_delete: false,
            folder_role: None,
            message: Message {
                id: message_id.clone(),
                account_id: account.clone(),
                session_id: SessionId::new("s1"),
                remote_key: None,
                location_status: None,
                location_revision: 0,
                internet_message_id: None,
                in_reply_to: None,
                subject: "with att".into(),
                body_preview: String::new(),
                body_content_type: BodyContentType::Text,
                body_content: Some(String::new()),
                from_address: None,
                from_name: String::new(),
                received_at: None,
                sent_at: Some(1),
                is_read: true,
                is_draft: false,
                remote_is_read: None,
                remote_is_draft: None,
                remote_is_deleted: None,
                is_outbound: true,
                has_attachments: true,
                importance: Importance::Normal,
                folder_id: None,
                parent_folder_name: None,
                local_updated_at: 1,
                remote_updated_at: None,
            },
            recipients: vec![],
        },
    )
    .await
    .unwrap();
    attachments::insert_message_attachment(
        &mut tx,
        &account,
        &message_id,
        &attachments::AttachmentRefRow {
            content_id: att.content_id.clone(),
            name: att.name.clone(),
            mime: att.mime.clone(),
            sort_order: 0,
            byte_length: att.size,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    // 删除草稿引用；消息仍引用同一 content_id
    let mut tx = db.begin().await.unwrap();
    attachments::delete_draft(&mut tx, &account, &SessionId::new("s1"))
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let removed =
        attachment_use_case::recycle_unreferenced(&db, &paths, &account, None)
            .await
            .unwrap();
    assert_eq!(removed, 0, "message ref must keep content alive");

    let bytes = attachment_use_case::load_content(&db, &paths, &account, &att.content_id)
        .await
        .unwrap();
    assert_eq!(bytes, b"\0\x01\x02payload");
    assert!(attachment_store::object_exists(
        &paths.attachments_dir,
        &account,
        &att.content_id
    ));
}

#[tokio::test]
async fn draft_and_binary_attachment_survive_restart() {
    let root = tempfile::tempdir().unwrap();
    let paths = paths(root.path());
    let db = db::open_sqlite(&paths.database_path).await.unwrap();
    seed_account_session(&db, "a", "s1").await;

    let payload = b"restart-binary-\xff\xfe";
    let att = attachment_use_case::import_attachment(
        &db,
        &paths,
        import_cmd("a", "s1", "photo.bin", payload),
    )
    .await
    .unwrap();

    let saved = compose_use_case::save_draft_with_refs(
        &db,
        &paths,
        SaveComposeDraftCommand {
            account_id: AccountId::new("a"),
            session_id: SessionId::new("s1"),
            reply_to_message_id: None,
            to: vec![],
            cc: vec![],
            bcc: vec![],
            subject: "草稿主题".into(),
            body_html: "<p>hello</p>".into(),
            attachments: vec![ComposeAttachmentDraft {
                content_id: att.content_id.clone(),
                name: att.name.clone(),
                size: att.size,
                mime: att.mime.clone(),
            }],
        },
    )
    .await
    .unwrap();
    assert_eq!(saved.subject, "草稿主题");
    assert_eq!(saved.attachments.len(), 1);
    db.close().await;

    // 模拟进程重启：新连接 + 同一附件目录
    let db2 = db::open_sqlite(&paths.database_path).await.unwrap();
    let draft = compose_use_case::get_draft(&db2, &AccountId::new("a"), &SessionId::new("s1"))
        .await
        .unwrap()
        .expect("draft must persist");
    assert_eq!(draft.subject, "草稿主题");
    assert_eq!(draft.body_html, "<p>hello</p>");
    assert_eq!(draft.attachments.len(), 1);
    assert_eq!(draft.attachments[0].content_id, att.content_id);
    assert_eq!(draft.attachments[0].name, "photo.bin");

    let bytes = attachment_use_case::load_content(
        &db2,
        &paths,
        &AccountId::new("a"),
        &att.content_id,
    )
    .await
    .unwrap();
    assert_eq!(bytes, payload);
}

#[tokio::test]
async fn missing_content_file_returns_visible_error() {
    let root = tempfile::tempdir().unwrap();
    let paths = paths(root.path());
    let db = db::open_memory().await.unwrap();
    seed_account_session(&db, "a", "s1").await;

    let att = attachment_use_case::import_attachment(
        &db,
        &paths,
        import_cmd("a", "s1", "gone.bin", b"data"),
    )
    .await
    .unwrap();

    attachment_store::delete_object(
        &paths.attachments_dir,
        &AccountId::new("a"),
        &att.content_id,
    )
    .unwrap();

    let err = attachment_use_case::load_content(
        &db,
        &paths,
        &AccountId::new("a"),
        &att.content_id,
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, AppErrorCode::Internal);
    assert!(err.message.contains("缺失") || err.message.contains("不可读"));
}

#[tokio::test]
async fn import_busy_lock_returns_retryable_conflict() {
    let root = tempfile::tempdir().unwrap();
    let paths = paths(root.path());
    let db = db::open_memory().await.unwrap();
    seed_account_session(&db, "a", "s1").await;

    let account = AccountId::new("a");
    let _held = attachment_store::lock_account(&paths.attachments_dir, &account).unwrap();

    let err = attachment_use_case::import_attachment(
        &db,
        &paths,
        import_cmd("a", "s1", "busy.bin", b"x"),
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, AppErrorCode::Conflict);
    assert!(err.retryable);
}

