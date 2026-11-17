use postlet_lib::application::dto::UpsertImapSmtpAccountCommand;
use postlet_lib::application::use_cases::{account_use_case, mail_account_use_case};
use postlet_lib::domain::value_objects::{MailAuthKind, MailTlsMode};
use postlet_lib::infrastructure::{
    db, mail::credentials, paths::AppPaths, repository::mail_transports, secret_store,
};

fn command() -> UpsertImapSmtpAccountCommand {
    UpsertImapSmtpAccountCommand {
        email: "user@example.com".into(),
        display_name: "User".into(),
        auth_kind: MailAuthKind::Password,
        imap_host: "imap.example.com".into(),
        imap_port: 993,
        imap_tls: MailTlsMode::Implicit,
        smtp_host: "smtp.example.com".into(),
        smtp_port: 587,
        smtp_tls: MailTlsMode::StartTls,
        username: None,
        password: Some(" password with spaces ".into()),
        access_token: None,
        refresh_token: None,
        expires_at_ms: None,
    }
}

fn paths(root: &std::path::Path) -> AppPaths {
    AppPaths {
        config_dir: root.join("config"),
        local_data_dir: root.into(),
        data_dir: root.join("data"),
        attachments_dir: root.join("attachments"),
        credentials_dir: root.join("credentials"),
        database_path: root.join("data/test.db"),
    }
}

#[tokio::test]
async fn invalid_configuration_has_no_database_or_filesystem_side_effects() {
    let db = db::open_memory().await.unwrap();
    let root = tempfile::tempdir().unwrap();
    let paths = paths(root.path());
    let mut cases = Vec::new();
    let mut cmd = command();
    cmd.auth_kind = MailAuthKind::OAuth2;
    cmd.access_token = Some("token".into());
    cases.push(cmd);
    let mut cmd = command();
    cmd.password = None;
    cases.push(cmd);
    let mut cmd = command();
    cmd.password = Some(String::new());
    cases.push(cmd);
    let mut cmd = command();
    cmd.imap_host = "https://example.com".into();
    cases.push(cmd);
    let mut cmd = command();
    cmd.smtp_port = 0;
    cases.push(cmd);
    let mut cmd = command();
    cmd.smtp_tls = MailTlsMode::Plain;
    cases.push(cmd);
    let mut cmd = command();
    cmd.imap_tls = MailTlsMode::Plain;
    cases.push(cmd);
    let mut cmd = command();
    cmd.username = Some("\r\n".into());
    cases.push(cmd);
    for cmd in cases {
        assert!(
            mail_account_use_case::upsert_imap_smtp_account(&db, &paths, cmd)
                .await
                .is_err()
        );
        assert!(!paths.credentials_dir.exists());
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
}

#[tokio::test]
#[ignore = "writes an isolated temporary OS keyring entry"]
async fn native_credentials_survive_failed_updates_and_recover_orphans() {
    let root = tempfile::tempdir().unwrap();
    let paths = paths(root.path());
    let db = db::open_memory().await.unwrap();
    let task_paths = paths.clone();
    let result = async move {
        let paths = task_paths;
        let account = mail_account_use_case::upsert_imap_smtp_account(&db, &paths, command())
            .await
            .unwrap();
        let original = mail_transports::get(&mut *db.acquire().await.unwrap(), &account.id)
            .await
            .unwrap()
            .unwrap();
        let secret = credentials::load(&paths.credentials_dir, &original)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(secret.password.as_deref(), Some(" password with spaces "));
        let original_ref = original.credential_ref.as_deref().unwrap();

        // 保存新秘密失败时数据库尚未改变。
        let mut broken_paths = paths.clone();
        let blocked = root.path().join("not-a-directory");
        std::fs::write(&blocked, b"blocked").unwrap();
        broken_paths.credentials_dir = blocked;
        assert!(
            mail_account_use_case::upsert_imap_smtp_account(&db, &broken_paths, command())
                .await
                .is_err()
        );

        // 凭据已暂存后，注入 SQL 写失败，必须清理新引用并保留旧凭据。
        sqlx::query("CREATE TRIGGER fail_config BEFORE UPDATE ON account_mail_transport BEGIN SELECT RAISE(ABORT, 'test failure'); END")
            .execute(&db).await.unwrap();
        assert!(
            mail_account_use_case::upsert_imap_smtp_account(&db, &paths, command())
                .await
                .is_err()
        );
        sqlx::query("DROP TRIGGER fail_config")
            .execute(&db)
            .await
            .unwrap();
        assert_eq!(
            secret_store::list_ids(&paths.credentials_dir, "mail").unwrap(),
            vec![original_ref]
        );
        let current = mail_transports::get(&mut *db.acquire().await.unwrap(), &account.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(current, original);

        // 延迟外键约束直到 COMMIT 失败，覆盖仅回滚写语句不足的场景。
        sqlx::query("CREATE TABLE commit_failure (id TEXT REFERENCES accounts(id) DEFERRABLE INITIALLY DEFERRED)").execute(&db).await.unwrap();
        sqlx::query("CREATE TRIGGER fail_commit AFTER UPDATE ON account_mail_transport BEGIN INSERT INTO commit_failure VALUES ('missing-account'); END")
            .execute(&db).await.unwrap();
        assert!(
            mail_account_use_case::upsert_imap_smtp_account(&db, &paths, command())
                .await
                .is_err()
        );
        sqlx::query("DROP TRIGGER fail_commit")
            .execute(&db)
            .await
            .unwrap();
        assert_eq!(
            secret_store::list_ids(&paths.credentials_dir, "mail").unwrap(),
            vec![original_ref]
        );
        assert_eq!(
            credentials::load(&paths.credentials_dir, &original)
                .await
                .unwrap()
                .unwrap()
                .password,
            secret.password
        );

        // 模拟暂存后崩溃及切换后旧文件残留，恢复只清理无引用文件。
        credentials::save(&paths.credentials_dir, "orphan", &secret)
            .await
            .unwrap();
        mail_account_use_case::recover_credentials(&db, &paths)
            .await
            .unwrap();
        assert_eq!(
            secret_store::list_ids(&paths.credentials_dir, "mail").unwrap(),
            vec![original_ref]
        );

        let updated = mail_account_use_case::upsert_imap_smtp_account(&db, &paths, command())
            .await
            .unwrap();
        assert_eq!(updated.id, account.id);
        let current = mail_transports::get(&mut *db.acquire().await.unwrap(), &account.id)
            .await
            .unwrap()
            .unwrap();
        assert_ne!(current.credential_ref, original.credential_ref);
        assert_eq!(
            secret_store::list_ids(&paths.credentials_dir, "mail").unwrap(),
            vec![current.credential_ref.clone().unwrap()]
        );
        assert!(
            credentials::load(&paths.credentials_dir, &original)
                .await
                .unwrap()
                .is_none()
        );

        // 同一账号更换密码保留位置；更换收信身份/端点必须原子退役旧位置和任务。
        sqlx::query("INSERT INTO mail_folder_sync_state(account_id,folder_id,protocol,remote_binding,display_name,sync_enabled,generation,updated_at) VALUES (?, 'test-folder', 'imap', 'INBOX', 'Inbox', 1, '1', 1)")
            .bind(account.id.as_str()).execute(&db).await.unwrap();
        sqlx::query("INSERT INTO sessions(id,account_id,kind,normalized_address,created_at,updated_at) VALUES ('scope-session',?,'private','peer@example.com',1,1)")
            .bind(account.id.as_str()).execute(&db).await.unwrap();
        sqlx::query("INSERT INTO messages(id,account_id,session_id,remote_key,location_status,location_revision,folder_id,body_content,local_updated_at) VALUES ('scope-message',?,'scope-session','imap:1:1','present',1,'test-folder','retained body',1)")
            .bind(account.id.as_str()).execute(&db).await.unwrap();
        let message_id = postlet_lib::domain::value_objects::MessageId::new("scope-message");
        let folder_id = postlet_lib::domain::value_objects::FolderId::new("test-folder");
        use postlet_lib::infrastructure::repository::{folder_states, messages, mutations};
        mutations::put_read(&mut *db.acquire().await.unwrap(), &account.id, &message_id, true).await.unwrap();
        mail_account_use_case::upsert_imap_smtp_account(&db, &paths, command()).await.unwrap();
        let folder = folder_states::get(&mut *db.acquire().await.unwrap(), &account.id, &folder_id).await.unwrap().unwrap();
        assert!(!folder.is_retired); assert_eq!(folder.binding_revision, 1);
        let mut changed_endpoint = command(); changed_endpoint.imap_host = "other-imap.example.com".into();
        sqlx::query("CREATE TRIGGER fail_endpoint_update BEFORE UPDATE ON account_mail_transport BEGIN SELECT RAISE(ABORT, 'test endpoint rollback'); END").execute(&db).await.unwrap();
        assert!(mail_account_use_case::upsert_imap_smtp_account(&db, &paths, changed_endpoint.clone()).await.is_err());
        sqlx::query("DROP TRIGGER fail_endpoint_update").execute(&db).await.unwrap();
        let folder = folder_states::get(&mut *db.acquire().await.unwrap(), &account.id, &folder_id).await.unwrap().unwrap();
        assert!(!folder.is_retired); assert!(folder.sync_enabled); assert_eq!(folder.generation.as_deref(), Some("1"));
        let pending = mutations::get(&mut *db.acquire().await.unwrap(), &account.id, &message_id).await.unwrap().unwrap();
        assert!(pending.is_active); assert!(pending.last_error.is_none());
        let retained = messages::get(&mut *db.acquire().await.unwrap(), &account.id, &message_id).await.unwrap().unwrap();
        assert_eq!(retained.message.location_status, Some(postlet_lib::domain::mail::LocationStatus::Present));
        mail_account_use_case::upsert_imap_smtp_account(&db, &paths, changed_endpoint).await.unwrap();
        let folder = folder_states::get(&mut *db.acquire().await.unwrap(), &account.id, &folder_id).await.unwrap().unwrap();
        assert!(folder.is_retired); assert_eq!(folder.binding_revision, 2);
        let pending = mutations::get(&mut *db.acquire().await.unwrap(), &account.id, &message_id).await.unwrap().unwrap();
        assert!(!pending.is_active); assert!(pending.last_error.is_some());
        let retained = messages::get(&mut *db.acquire().await.unwrap(), &account.id, &message_id).await.unwrap().unwrap();
        assert_eq!(retained.message.body_content.as_deref(), Some("retained body"));
        assert_eq!(retained.message.location_status, Some(postlet_lib::domain::mail::LocationStatus::Invalidated));
        let current = mail_transports::get(&mut *db.acquire().await.unwrap(), &account.id).await.unwrap().unwrap();

        // 删除失败应可见、可重试，不能先改为已退出。
        let file = std::fs::read_dir(&paths.credentials_dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.extension().is_some_and(|ext| ext == "secret"))
            .unwrap();
        std::fs::remove_file(&file).unwrap();
        std::fs::create_dir(&file).unwrap();
        assert!(
            account_use_case::sign_out(&db, &paths, &account.id)
                .await
                .is_err()
        );
        let signed_in: bool = sqlx::query_scalar("SELECT is_signed_in FROM accounts WHERE id = ?")
            .bind(account.id.as_str())
            .fetch_one(&db)
            .await
            .unwrap();
        assert!(signed_in);
        std::fs::remove_dir(&file).unwrap();
        credentials::save(
            &paths.credentials_dir,
            current.credential_ref.as_deref().unwrap(),
            &secret,
        )
        .await
        .unwrap();
        account_use_case::sign_out(&db, &paths, &account.id)
            .await
            .unwrap();
        let signed_in: bool = sqlx::query_scalar("SELECT is_signed_in FROM accounts WHERE id = ?")
            .bind(account.id.as_str())
            .fetch_one(&db)
            .await
            .unwrap();
        assert!(!signed_in);
    };
    // 即使测试断言失败也回收隔离系统密钥。
    let task = tokio::spawn(result).await;
    if let Ok(canonical) = std::fs::canonicalize(&paths.credentials_dir) {
        let _ = keyring::Entry::new("com.postlet.credentials", &canonical.to_string_lossy())
            .unwrap()
            .delete_credential();
    }
    task.unwrap();
}
