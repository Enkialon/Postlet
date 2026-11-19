//! 垃圾邮件阅读：三态文件夹设置、默认列表可见性、单封移动目标与意图冻结。

use postlet_lib::application::dto::{
    MoveMessageJunkCommand, SetFolderJunkOverrideCommand, SessionListQuery,
};
use postlet_lib::application::use_cases::{message_use_case, session_use_case, sync_use_case};
use postlet_lib::application::AppErrorCode;
use postlet_lib::domain::entities::{JunkMoveDirection, PendingMutationKind};
use postlet_lib::domain::mail::DiscoveredFolder;
use postlet_lib::domain::services::FolderRole;
use postlet_lib::domain::value_objects::{AccountId, FolderId, MessageId, ReceiveProtocol};
use postlet_lib::infrastructure::db;
use postlet_lib::infrastructure::repository::{folder_states, messages, mutations, sessions};
use sqlx::SqlitePool;

async fn seed_account(db: &SqlitePool, account: &str) {
    sqlx::query("INSERT INTO accounts(id,email,created_at,updated_at) VALUES (?,?,1,1)")
        .bind(account)
        .bind(format!("{account}@example.com"))
        .execute(db)
        .await
        .unwrap();
}

/// 直接写文件夹行（模拟已发现目录后的状态）
async fn seed_folder(
    db: &SqlitePool,
    account: &str,
    folder: &str,
    binding: &str,
    role: &str,
    source: &str,
) {
    sqlx::query(
        "INSERT INTO mail_folder_sync_state(
            account_id,folder_id,protocol,remote_binding,binding_revision,is_retired,
            display_name,sync_enabled,sync_status,updated_at,role,role_source)
         VALUES (?,?,'imap',?,1,0,?,1,'idle',1,?,?)",
    )
    .bind(account)
    .bind(folder)
    .bind(binding)
    .bind(folder)
    .bind(role)
    .bind(source)
    .execute(db)
    .await
    .unwrap();
}

async fn seed_session(db: &SqlitePool, account: &str, session: &str) {
    sqlx::query(
        "INSERT INTO sessions(account_id,id,kind,normalized_address,created_at,updated_at)
         VALUES (?,?,'private',?,1,1)",
    )
    .bind(account)
    .bind(session)
    .bind(format!("{session}@example.com"))
    .execute(db)
    .await
    .unwrap();
}

/// 现场存在的邮件：占一个真实远端位置
async fn seed_message(
    db: &SqlitePool,
    account: &str,
    session: &str,
    message: &str,
    folder: &str,
    remote_key: &str,
) {
    sqlx::query(
        "INSERT INTO messages(
            account_id,id,session_id,remote_key,location_status,location_revision,folder_id,
            subject,body_preview,body_content_type,from_name,is_read,is_draft,is_outbound,
            has_attachments,importance,local_updated_at)
         VALUES (?,?,?,?,'present',1,?,'s','preview','text','',0,0,0,0,'normal',1)",
    )
    .bind(account)
    .bind(message)
    .bind(session)
    .bind(remote_key)
    .bind(folder)
    .execute(db)
    .await
    .unwrap();
}

fn discovered(entries: &[(&str, &str, Option<FolderRole>)]) -> Vec<DiscoveredFolder> {
    entries
        .iter()
        .map(|(binding, display_name, server_role)| DiscoveredFolder {
            server_role: *server_role,
            remote_binding: (*binding).to_string(),
            parent_binding: None,
            display_name: (*display_name).to_string(),
        })
        .collect()
}

async fn folder_of(db: &SqlitePool, account: &str, folder: &str) -> FolderId {
    let id: String = sqlx::query_scalar(
        "SELECT folder_id FROM mail_folder_sync_state WHERE account_id = ? AND display_name = ?",
    )
    .bind(account)
    .bind(folder)
    .fetch_one(db)
    .await
    .unwrap();
    FolderId::new(id)
}

#[tokio::test]
async fn junk_override_is_three_state_and_survives_rediscovery() {
    let db = db::open_memory().await.unwrap();
    seed_account(&db, "a").await;
    let mut conn = db.acquire().await.unwrap();
    let tree = discovered(&[
        ("INBOX", "INBOX", Some(FolderRole::Inbox)),
        ("Junk", "Junk", Some(FolderRole::Junk)),
    ]);
    folder_states::reconcile_discovered(&mut conn, &AccountId::new("a"), ReceiveProtocol::Imap, &tree, 1)
        .await
        .unwrap();
    drop(conn);

    let junk = folder_of(&db, "a", "Junk").await;
    let inbox = folder_of(&db, "a", "INBOX").await;

    // 自动识别：服务器 \Junk → 生效 junk
    let progress = |folder: FolderId| {
        let db = db.clone();
        async move {
            let mut conn = db.acquire().await.unwrap();
            let state = folder_states::get(&mut conn, &AccountId::new("a"), &folder)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(state.role, FolderRole::Junk);
            assert_eq!(state.role_source, postlet_lib::domain::services::RoleSource::Server);
            state
        }
    };
    assert_eq!(progress(junk.clone()).await.effective_role, FolderRole::Junk);

    // 手动否定：生效 custom，自动识别结果与来源保留
    let (updated, changes) = sync_use_case::set_folder_junk_override(
        &db,
        SetFolderJunkOverrideCommand {
            account_id: AccountId::new("a"),
            folder_id: junk.clone(),
            junk_override: Some(false),
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.effective_role, FolderRole::Custom);
    assert_eq!(updated.junk_override, Some(false));
    assert_eq!(updated.role, FolderRole::Junk);
    // 目录内已有邮件才需要通知；此处没有邮件，不产生空事件
    assert!(changes.is_none());

    // 重新发现目录不覆盖手动否定
    let mut conn = db.acquire().await.unwrap();
    folder_states::reconcile_discovered(&mut conn, &AccountId::new("a"), ReceiveProtocol::Imap, &tree, 2)
        .await
        .unwrap();
    let after = folder_states::get(&mut conn, &AccountId::new("a"), &junk)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.junk_override, Some(false));
    assert_eq!(after.effective_role, FolderRole::Custom);

    // 同步进度 upsert 也不写覆盖列
    let mut saved = after.clone();
    saved.junk_override = Some(true);
    saved.sync_cursor = Some("cursor".into());
    folder_states::upsert(&mut conn, &saved).await.unwrap();
    let reread = folder_states::get(&mut conn, &AccountId::new("a"), &junk)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reread.junk_override, Some(false), "同步/发现 upsert 不得写覆盖列");
    assert_eq!(reread.effective_role, FolderRole::Custom);
    drop(conn);

    // 切回自动恢复协议结果
    let (restored, _) = sync_use_case::set_folder_junk_override(
        &db,
        SetFolderJunkOverrideCommand {
            account_id: AccountId::new("a"),
            folder_id: junk.clone(),
            junk_override: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(restored.effective_role, FolderRole::Junk);

    // 非垃圾角色的「不是」不会把它变成 custom
    let (plain, _) = sync_use_case::set_folder_junk_override(
        &db,
        SetFolderJunkOverrideCommand {
            account_id: AccountId::new("a"),
            folder_id: inbox.clone(),
            junk_override: Some(false),
        },
    )
    .await
    .unwrap();
    assert_eq!(plain.effective_role, FolderRole::Inbox);
    // 手动指定：普通夹可被标为垃圾
    let (marked, _) = sync_use_case::set_folder_junk_override(
        &db,
        SetFolderJunkOverrideCommand {
            account_id: AccountId::new("a"),
            folder_id: inbox.clone(),
            junk_override: Some(true),
        },
    )
    .await
    .unwrap();
    assert_eq!(marked.effective_role, FolderRole::Junk);
    assert_eq!(marked.role, FolderRole::Inbox);

    // 跨账号 / 已退役文件夹拒绝写入
    seed_account(&db, "b").await;
    let missing = sync_use_case::set_folder_junk_override(
        &db,
        SetFolderJunkOverrideCommand {
            account_id: AccountId::new("b"),
            folder_id: junk.clone(),
            junk_override: Some(true),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(missing.code, AppErrorCode::NotFound);
    sqlx::query("UPDATE mail_folder_sync_state SET is_retired = 1 WHERE account_id='a' AND folder_id=?")
        .bind(junk.as_str())
        .execute(&db)
        .await
        .unwrap();
    let retired = sync_use_case::set_folder_junk_override(
        &db,
        SetFolderJunkOverrideCommand {
            account_id: AccountId::new("a"),
            folder_id: junk,
            junk_override: Some(true),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(retired.code, AppErrorCode::NotFound);
}

#[tokio::test]
async fn junk_only_session_is_visible_by_default_while_trash_stays_hidden() {
    let db = db::open_memory().await.unwrap();
    seed_account(&db, "a").await;
    seed_folder(&db, "a", "INBOX", "INBOX", "inbox", "server").await;
    seed_folder(&db, "a", "Junk", "Junk", "junk", "server").await;
    seed_folder(&db, "a", "Trash", "Trash", "trash", "server").await;

    seed_session(&db, "a", "s-junk").await;
    seed_session(&db, "a", "s-trash").await;
    seed_message(&db, "a", "s-junk", "m-junk", "Junk", "imap:1:11").await;
    seed_message(&db, "a", "s-trash", "m-trash", "Trash", "imap:1:12").await;

    let mut conn = db.acquire().await.unwrap();
    for session in ["s-junk", "s-trash"] {
        sessions::recompute_projection(&mut conn, &AccountId::new("a"), &sqlx_session(session))
            .await
            .unwrap();
    }
    drop(conn);

    // 默认列表：仅含垃圾邮件的会话可见，Trash 仍排除
    let page = session_use_case::list_sessions(
        &db,
        SessionListQuery {
            account_id: Some(AccountId::new("a")),
            cursor: None,
            limit: 20,
            kind: None,
            include_excluded_folders: false,
        },
    )
    .await
    .unwrap();
    let ids: Vec<&str> = page.items.iter().map(|i| i.session.id.as_str()).collect();
    assert_eq!(ids, vec!["s-junk"]);
    let preview = page.items[0].last_message_preview.as_ref().unwrap();
    assert_eq!(preview.folder_role, Some(FolderRole::Junk));
    assert_eq!(preview.body_preview, "preview", "垃圾邮件也要有文本摘要");
    // 未读计数策略不变：垃圾夹未读不计入
    assert_eq!(page.items[0].session.unread_count, 0);

    // 消息级投影使用同一生效角色
    let mut conn = db.acquire().await.unwrap();
    let bundle = messages::get(&mut conn, &AccountId::new("a"), &MessageId::new("m-junk"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(bundle.folder_role, Some(FolderRole::Junk));
    drop(conn);

    // 手动否定后：同一封邮件按生效角色显示为普通夹，列表仍可见
    let junk = folder_of(&db, "a", "Junk").await;
    sync_use_case::set_folder_junk_override(
        &db,
        SetFolderJunkOverrideCommand {
            account_id: AccountId::new("a"),
            folder_id: junk,
            junk_override: Some(false),
        },
    )
    .await
    .unwrap();
    let mut conn = db.acquire().await.unwrap();
    let bundle = messages::get(&mut conn, &AccountId::new("a"), &MessageId::new("m-junk"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(bundle.folder_role, Some(FolderRole::Custom));
    drop(conn);
    let page = session_use_case::list_sessions(
        &db,
        SessionListQuery {
            account_id: Some(AccountId::new("a")),
            cursor: None,
            limit: 20,
            kind: None,
            include_excluded_folders: false,
        },
    )
    .await
    .unwrap();
    assert_eq!(page.items.len(), 1);
}

fn sqlx_session(id: &str) -> postlet_lib::domain::value_objects::SessionId {
    postlet_lib::domain::value_objects::SessionId::new(id)
}

#[tokio::test]
async fn single_message_junk_move_freezes_source_and_requires_a_unique_target() {
    let db = db::open_memory().await.unwrap();
    seed_account(&db, "a").await;
    seed_folder(&db, "a", "INBOX", "INBOX", "inbox", "server").await;
    seed_session(&db, "a", "s1").await;
    seed_message(&db, "a", "s1", "m1", "INBOX", "imap:1:20").await;

    // 没有垃圾文件夹 → 明确要求先配置，不按名称随便挑
    let error = message_use_case::move_message_junk(
        &db,
        MoveMessageJunkCommand {
            account_id: AccountId::new("a"),
            message_id: MessageId::new("m1"),
            direction: JunkMoveDirection::ToJunk,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, AppErrorCode::Validation);
    {
        let mut conn = db.acquire().await.unwrap();
        assert!(
            mutations::get(&mut conn, &AccountId::new("a"), &MessageId::new("m1"))
                .await
                .unwrap()
                .is_none()
        );
    }

    // 协议确认的标准垃圾夹 + 手动候选：优先标准夹
    seed_folder(&db, "a", "Spam", "Spam", "junk", "guess").await;
    seed_folder(&db, "a", "Junk", "Junk", "junk", "server").await;
    message_use_case::move_message_junk(
        &db,
        MoveMessageJunkCommand {
            account_id: AccountId::new("a"),
            message_id: MessageId::new("m1"),
            direction: JunkMoveDirection::ToJunk,
        },
    )
    .await
    .unwrap();
    let junk = folder_of(&db, "a", "Junk").await;
    let inbox = folder_of(&db, "a", "INBOX").await;
    {
        let mut conn = db.acquire().await.unwrap();
        let saved = mutations::get(&mut conn, &AccountId::new("a"), &MessageId::new("m1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.kind, PendingMutationKind::Junk);
        assert_eq!(saved.destination_folder_id, Some(junk.clone()));
        assert_eq!(saved.target_folder_id, inbox);
        assert_eq!(saved.target_remote_key.as_str(), "imap:1:20");
        // 只有选中这一封：会话里其它邮件不受影响（此处仅一封，验证范围参数是单封）
        let bundle = messages::get(&mut conn, &AccountId::new("a"), &MessageId::new("m1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(bundle.pending_junk_move, Some(JunkMoveDirection::ToJunk));
        assert_eq!(bundle.folder_role, Some(FolderRole::Inbox), "本地位置事实不改");
    }

    // 再来一个服务端确认的垃圾夹 → 歧义，必须失败
    seed_folder(&db, "a", "Junk2", "Junk2", "junk", "server").await;
    sqlx::query("DELETE FROM pending_message_mutations WHERE account_id='a' AND message_id='m1'")
        .execute(&db)
        .await
        .unwrap();
    let error = message_use_case::move_message_junk(
        &db,
        MoveMessageJunkCommand {
            account_id: AccountId::new("a"),
            message_id: MessageId::new("m1"),
            direction: JunkMoveDirection::ToJunk,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, AppErrorCode::Validation);

    // 移出垃圾：目标必须是唯一收件箱
    seed_session(&db, "a", "s2").await;
    seed_message(&db, "a", "s2", "m2", "Junk", "imap:1:21").await;
    message_use_case::move_message_junk(
        &db,
        MoveMessageJunkCommand {
            account_id: AccountId::new("a"),
            message_id: MessageId::new("m2"),
            direction: JunkMoveDirection::OutOfJunk,
        },
    )
    .await
    .unwrap();
    let out = {
        let mut conn = db.acquire().await.unwrap();
        mutations::get(&mut conn, &AccountId::new("a"), &MessageId::new("m2"))
            .await
            .unwrap()
            .unwrap()
    };
    assert_eq!(out.kind, PendingMutationKind::Unjunk);
    assert_eq!(out.destination_folder_id, Some(folder_of(&db, "a", "INBOX").await));

    // 两个收件箱候选 → 不猜
    seed_folder(&db, "a", "Inbox2", "Inbox2", "inbox", "guess").await;
    let error = message_use_case::move_message_junk(
        &db,
        MoveMessageJunkCommand {
            account_id: AccountId::new("a"),
            message_id: MessageId::new("m2"),
            direction: JunkMoveDirection::OutOfJunk,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, AppErrorCode::Validation);
}

#[tokio::test]
async fn local_send_records_and_pending_deletes_cannot_be_moved() {
    let db = db::open_memory().await.unwrap();
    seed_account(&db, "a").await;
    seed_folder(&db, "a", "INBOX", "INBOX", "inbox", "server").await;
    seed_folder(&db, "a", "Junk", "Junk", "junk", "server").await;
    seed_session(&db, "a", "s1").await;
    // 本地发信记录：没有远端位置
    sqlx::query(
        "INSERT INTO messages(
            account_id,id,session_id,location_status,subject,body_preview,body_content_type,
            from_name,is_read,is_draft,is_outbound,has_attachments,importance,local_updated_at)
         VALUES ('a','m-local','s1',NULL,'s','','text','',1,0,1,0,'normal',1)",
    )
    .execute(&db)
    .await
    .unwrap();
    let error = message_use_case::move_message_junk(
        &db,
        MoveMessageJunkCommand {
            account_id: AccountId::new("a"),
            message_id: MessageId::new("m-local"),
            direction: JunkMoveDirection::ToJunk,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, AppErrorCode::Validation);

    // 已有删除意图 → 冲突
    seed_message(&db, "a", "s1", "m1", "INBOX", "imap:1:30").await;
    let mut conn = db.acquire().await.unwrap();
    mutations::put_delete(&mut conn, &AccountId::new("a"), &MessageId::new("m1"))
        .await
        .unwrap();
    drop(conn);
    let error = message_use_case::move_message_junk(
        &db,
        MoveMessageJunkCommand {
            account_id: AccountId::new("a"),
            message_id: MessageId::new("m1"),
            direction: JunkMoveDirection::ToJunk,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, AppErrorCode::Conflict);
}

#[tokio::test]
async fn move_destination_must_still_be_junk_or_inbox_when_flushing() {
    let db = db::open_memory().await.unwrap();
    seed_account(&db, "a").await;
    seed_folder(&db, "a", "INBOX", "INBOX", "inbox", "server").await;
    seed_folder(&db, "a", "Junk", "Junk", "junk", "server").await;
    seed_session(&db, "a", "s1").await;
    seed_message(&db, "a", "s1", "m1", "INBOX", "imap:1:40").await;

    message_use_case::move_message_junk(
        &db,
        MoveMessageJunkCommand {
            account_id: AccountId::new("a"),
            message_id: MessageId::new("m1"),
            direction: JunkMoveDirection::ToJunk,
        },
    )
    .await
    .unwrap();
    let junk = folder_of(&db, "a", "Junk").await;
    let mut conn = db.acquire().await.unwrap();
    let saved = mutations::get(&mut conn, &AccountId::new("a"), &MessageId::new("m1"))
        .await
        .unwrap()
        .unwrap();
    let (binding, role) = mutations::resolve_move_destination(&mut conn, &saved, ReceiveProtocol::Imap)
        .await
        .unwrap()
        .expect("目标文件夹仍有效");
    assert_eq!(binding, "Junk");
    assert!(JunkMoveDirection::ToJunk.accepts(role));
    drop(conn);

    // 目标被手动取消为垃圾夹 → 方向不再成立
    sync_use_case::set_folder_junk_override(
        &db,
        SetFolderJunkOverrideCommand {
            account_id: AccountId::new("a"),
            folder_id: junk.clone(),
            junk_override: Some(false),
        },
    )
    .await
    .unwrap();
    let mut conn = db.acquire().await.unwrap();
    let (_, role) = mutations::resolve_move_destination(&mut conn, &saved, ReceiveProtocol::Imap)
        .await
        .unwrap()
        .expect("文件夹本身仍存活");
    assert!(!JunkMoveDirection::ToJunk.accepts(role));

    // 目标文件夹退役 → 无法解析，flush 必须明确失败
    sqlx::query("UPDATE mail_folder_sync_state SET is_retired = 1 WHERE account_id='a' AND folder_id=?")
        .bind(junk.as_str())
        .execute(&mut *conn)
        .await
        .unwrap();
    assert!(mutations::resolve_move_destination(&mut conn, &saved, ReceiveProtocol::Imap)
        .await
        .unwrap()
        .is_none());
}
