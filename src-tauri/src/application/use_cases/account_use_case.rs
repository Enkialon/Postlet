//! 账户用例
use crate::domain::entities::AccountMailTransport;
use crate::domain::value_objects::{AccountId, CredentialRef, EmailAddress, ReceiveProtocol, SyncStatus, TimestampMs};
use crate::infrastructure::config::load_azure_auth_config;
use crate::infrastructure::graph::auth as graph_auth;
use crate::infrastructure::graph::{TokenCache, models::RemoteAccount};
use crate::infrastructure::mail::credentials;
use crate::infrastructure::paths::AppPaths;
use crate::infrastructure::repository::{accounts, mail_transports};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::application::dto::{
    ChangeOrigin, ChangeSet, UpdateAccountAliasesCommand, UpdateAccountDisplaySettingsCommand,
};
use crate::application::error::AppError;
use crate::application::use_cases::aggregate_worker_use_case;
use crate::application::use_cases::compose_use_case;
use crate::domain::entities::Account;

/// 列出当前已登录的 Outlook 账户，供启动恢复和账户切换使用。
///
/// 已退出（仅清凭证）的账户行仍在库中，但不出现在此列表；不发起登录或远端同步。
pub async fn list_accounts(db: &SqlitePool) -> Result<Vec<Account>, AppError> {
    let mut conn = db.acquire().await?;
    accounts::list(&mut conn).await.map_err(Into::into)
}

/// 按本地账户 ID 获取账户资料；不存在时返回未找到错误。
pub async fn get_account(db: &SqlitePool, account_id: &AccountId) -> Result<Account, AppError> {
    let mut conn = db.acquire().await?;
    accounts::get(&mut conn, account_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("账户不存在: {account_id}")))
}

/// 浏览器认证不修改本地账号；Runtime 确认既有账号并排空旧任务后再提交。
pub(crate) struct PendingGraphSignIn {
    remote: RemoteAccount,
    token_cache: TokenCache,
}

pub(crate) async fn begin_graph_sign_in(paths: &AppPaths) -> Result<PendingGraphSignIn, AppError> {
    let config = load_azure_auth_config(&paths.config_dir).map_err(|e| {
        let msg = e.to_string();
        if msg.contains("未找到 Azure 配置") || msg.contains("未配置 Azure") {
            AppError::validation(msg)
        } else {
            AppError::from(e)
        }
    })?;

    let (remote, token_cache) = graph_auth::sign_in(&config).await?;
    Ok(PendingGraphSignIn {
        remote,
        token_cache,
    })
}

async fn find_graph_account(
    conn: &mut sqlx::SqliteConnection,
    remote: &RemoteAccount,
) -> Result<Option<Account>, AppError> {
    Ok(accounts::get_by_email(conn, &remote.email).await?)
}

pub(crate) async fn graph_sign_in_account(
    db: &SqlitePool,
    pending: &PendingGraphSignIn,
) -> Result<Option<AccountId>, AppError> {
    let mut conn = db.acquire().await?;
    Ok(find_graph_account(&mut conn, &pending.remote)
        .await?
        .map(|account| account.id))
}

pub(crate) async fn complete_graph_sign_in(
    db: &SqlitePool,
    paths: &AppPaths,
    pending: PendingGraphSignIn,
    expected_account: Option<&AccountId>,
) -> Result<Account, AppError> {
    let _lock = credentials::lock(&paths.credentials_dir)?;
    let reference = CredentialRef::new(Uuid::new_v4().to_string())?;
    // 新引用可读之前不改变登录态、协议、游标或旧凭据。
    graph_auth::save_token_cache(&paths.credentials_dir, &reference, &pending.token_cache).await?;
    match persist_graph_sign_in(db, &pending.remote, expected_account, &reference).await {
        Ok((account, previous)) => {
            if let Some(previous) = previous {
                credentials::delete_for_transport(&paths.credentials_dir, &previous)
                    .await
                    .map_err(|_| {
                        AppError::internal("Graph 登录已保存，但旧凭据清理失败；重启时会再次清理")
                    })?;
            }
            Ok(account)
        }
        Err(error) => {
            // COMMIT 应答失败不代表未提交；先确认没有引用，才删除暂存令牌。
            let referenced = match db.acquire().await {
                Ok(mut conn) => mail_transports::is_credential_referenced(&mut conn, &reference).await,
                Err(error) => Err(error.into()),
            };
            if matches!(referenced, Ok(false)) {
                crate::infrastructure::secret_store::delete(
                    &paths.credentials_dir,
                    "graph",
                    reference.as_str(),
                )
                .map_err(|_| {
                    AppError::internal("Graph 登录未完成，暂存令牌清理失败；重启时会再次清理")
                })?;
            }
            Err(error)
        }
    }
}

async fn persist_graph_sign_in(
    db: &SqlitePool,
    remote: &RemoteAccount,
    expected_account: Option<&AccountId>,
    reference: &CredentialRef,
) -> Result<(Account, Option<AccountMailTransport>), AppError> {
    let now = now_ms();
    let mut tx = db.begin().await?;
    let existing = find_graph_account(&mut tx, remote).await?;
    if existing.as_ref().map(|account| &account.id) != expected_account {
        return Err(AppError::conflict(
            "账号在登录期间已改变，请重试以停止对应旧任务",
        ));
    }
    let previous = match &existing {
        Some(account) => mail_transports::get(&mut tx, &account.id).await?,
        None => None,
    };
    let account = if let Some(existing) = existing {
        Account {
            id: existing.id,
            email: remote.email.clone(),
            display_name: remote.display_name.clone(),
            // 别名仅手动维护，登录刷新不得冲掉
            aliases: existing.aliases,
            contact_delta_link: existing.contact_delta_link,
            mail_sync_triggered_at: existing.mail_sync_triggered_at,
            contact_synced_at: existing.contact_synced_at,
            mail_sync_status: existing.mail_sync_status,
            mail_sync_error: existing.mail_sync_error,
            include_in_all: existing.include_in_all,
            created_at: existing.created_at,
            updated_at: now,
        }
    } else {
        Account {
            id: AccountId::new(Uuid::new_v4().to_string()),
            email: remote.email.clone(),
            display_name: remote.display_name.clone(),
            aliases: Vec::new(),
            contact_delta_link: None,
            mail_sync_triggered_at: None,
            contact_synced_at: None,
            mail_sync_status: SyncStatus::Idle,
            mail_sync_error: None,
            include_in_all: true,
            created_at: now,
            updated_at: now,
        }
    };

    accounts::upsert_identity(&mut tx, &account).await?;
    if previous
        .as_ref()
        .is_some_and(|transport| transport.receive.protocol() != ReceiveProtocol::Graph)
    {
        crate::application::use_cases::mail_account_use_case::retire_mail_identity(
            &mut tx,
            &account.id,
            now,
            "收信协议已切换为 Graph，旧操作已停止",
        )
        .await?;
    }
    mail_transports::upsert(
        &mut tx,
        &AccountMailTransport::graph(account.id.clone(), reference.clone(), now),
    )
    .await?;
    let saved = accounts::get(&mut tx, &account.id)
        .await?
        .ok_or_else(|| AppError::internal("登录后读取账户失败"))?;
    tx.commit().await?;

    Ok((saved, previous))
}

/// 退出指定账户：删除本地凭证并标记未登录；保留账户行与本地邮件。
///
/// 再登录同一微软账户会复用原 `AccountId` 与本地数据，再拉取离线期间邮件。
/// 列表在 `account_id` 为空时只 `IN` 已登录且 `include_in_all` 的账号。
pub async fn sign_out(
    db: &SqlitePool,
    paths: &AppPaths,
    account_id: &AccountId,
) -> Result<(), AppError> {
    let mut conn = db.acquire().await?;
    let _ = accounts::get(&mut conn, account_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("账户不存在: {account_id}")))?;
    let _credential_lock = credentials::lock(&paths.credentials_dir)?;
    if let Some(transport) = mail_transports::get(&mut conn, account_id).await? {
        credentials::delete_for_transport(&paths.credentials_dir, &transport).await?;
    }
    accounts::set_signed_in(&mut conn, account_id, false)
        .await
        .map_err(AppError::from)?;
    Ok(())
}

/// 清理指定账号的本地聊天/同步数据；保留账号行、登录态与传输配置。
///
/// 与 `sign_out` 相反侧重：不删凭据、不改 `is_signed_in`；删会话/邮件/联系人/群与同步游标。
/// Runtime 先暂停并等待旧任务退出，避免与写入竞态。
pub async fn clear_local_data(
    db: &SqlitePool,
    paths: &AppPaths,
    account_id: &AccountId,
) -> Result<ChangeSet, AppError> {
    {
        let mut conn = db.acquire().await?;
        let _ = accounts::get(&mut conn, account_id)
            .await?
            .ok_or_else(|| AppError::not_found(format!("账户不存在: {account_id}")))?;
    }

    let mut tx = db.begin().await?;
    accounts::clear_local_chat_data(&mut tx, account_id)
        .await
        .map_err(AppError::from)?;
    tx.commit().await?;

    compose_use_case::clear_drafts_for_account(db, paths, account_id).await?;

    let mut changeset = ChangeSet::empty(account_id.clone(), ChangeOrigin::UserAction);
    changeset.account_cleared = true;
    Ok(changeset)
}

/// 更新账户显示配置（是否纳入「全部账号」列表）。
pub async fn update_display_settings(
    db: &SqlitePool,
    cmd: UpdateAccountDisplaySettingsCommand,
) -> Result<Account, AppError> {
    let mut tx = db.begin().await?;
    accounts::update_display_settings(&mut tx, &cmd.account_id, cmd.include_in_all)
        .await
        .map_err(AppError::from)?;
    let account = accounts::get(&mut tx, &cmd.account_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("账户不存在: {}", cmd.account_id)))?;
    tx.commit().await?;
    Ok(account)
}

/// 手动更新账户别名（self 额外身份；整表替换）。
///
/// 集合变化时：把「对端已变 self」的私聊消息迁到正确会话（读路径动态判定 is_outbound）。
pub async fn update_aliases(
    db: &SqlitePool,
    cmd: UpdateAccountAliasesCommand,
) -> Result<(Account, Option<ChangeSet>), AppError> {
    let mut tx = db.begin().await?;
    let account = accounts::get(&mut tx, &cmd.account_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("账户不存在: {}", cmd.account_id)))?;

    let aliases = normalize_aliases(&account.email, &cmd.aliases)?;
    let aliases_changed = aliases != account.aliases;
    ensure_aliases_not_claimed(&mut tx, &account.id, &aliases).await?;

    accounts::update_aliases(&mut tx, &cmd.account_id, &aliases)
        .await
        .map_err(AppError::from)?;
    let updated = accounts::get(&mut tx, &cmd.account_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("账户不存在: {}", cmd.account_id)))?;
    tx.commit().await?;

    let changes = if aliases_changed {
        let (_, changeset) =
            aggregate_worker_use_case::relocate_self_keyed_private_sessions(db, &updated.id).await?;
        changeset
    } else {
        None
    };
    Ok((updated, changes))
}

fn normalize_aliases(
    primary: &EmailAddress,
    raw: &[String],
) -> Result<Vec<EmailAddress>, AppError> {
    let mut out = Vec::new();
    for item in raw {
        let trimmed = item.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !trimmed.contains('@') {
            return Err(AppError::validation(format!("邮箱地址无效: {trimmed}")));
        }
        let Some(addr) = EmailAddress::parse(trimmed) else {
            continue;
        };
        if &addr == primary {
            return Err(AppError::validation("别名不能与主邮箱相同".to_string()));
        }
        if out.iter().any(|a| a == &addr) {
            continue;
        }
        out.push(addr);
    }
    Ok(out)
}

async fn ensure_aliases_not_claimed(
    conn: &mut sqlx::SqliteConnection,
    account_id: &AccountId,
    aliases: &[EmailAddress],
) -> Result<(), AppError> {
    if aliases.is_empty() {
        return Ok(());
    }
    let others = accounts::list_all(conn).await.map_err(AppError::from)?;
    for other in others {
        if &other.id == account_id {
            continue;
        }
        for alias in aliases {
            if alias == &other.email {
                return Err(AppError::validation(format!(
                    "别名已被其他账号主邮箱占用: {alias}"
                )));
            }
            if other.aliases.iter().any(|a| a == alias) {
                return Err(AppError::validation(format!(
                    "别名已被其他账号使用: {alias}"
                )));
            }
        }
    }
    Ok(())
}

fn now_ms() -> TimestampMs {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod graph_sign_in_tests {
    use super::*;
    use crate::application::dto::{
        ImapAuthenticationInput, SmtpAuthenticationInput, UpsertImapSmtpAccountCommand,
    };
    use crate::application::use_cases::mail_account_use_case;
    use crate::domain::entities::{GraphAuthConfig, ReceiveConfig};
    use crate::domain::value_objects::{MailTlsMode, SendProtocol};
    use crate::infrastructure::{db, secret_store};

    fn pending(token: &str) -> PendingGraphSignIn {
        PendingGraphSignIn {
            remote: RemoteAccount {
                account_id: "provider-only".into(),
                email: EmailAddress::parse("user@example.com").unwrap(),
                display_name: "Graph User".into(),
                aliases: vec![],
            },
            token_cache: TokenCache {
                access_token: token.into(),
                refresh_token: Some("refresh".into()),
                expires_at_ms: now_ms() + 3_600_000,
                scopes: vec!["Mail.ReadWrite".into()],
                home_account_id: Some("home.tenant".into()),
                client_id: "test-client".into(),
                tenant: "tenant".into(),
            },
        }
    }

    #[tokio::test]
    async fn graph_account_lookup_uses_normalized_email_not_provider_fields() {
        let db = db::open_memory().await.unwrap();
        let mut conn = db.acquire().await.unwrap();
        let now = now_ms();
        let existing = Account {
            id: AccountId::new("acc-existing"),
            email: EmailAddress::parse("user@example.com").unwrap(),
            display_name: "Local".into(),
            aliases: vec![],
            contact_delta_link: None,
            mail_sync_triggered_at: None,
            contact_synced_at: None,
            mail_sync_status: SyncStatus::Idle,
            mail_sync_error: None,
            include_in_all: true,
            created_at: now,
            updated_at: now,
        };
        accounts::upsert(&mut conn, &existing).await.unwrap();

        let same_email = RemoteAccount {
            account_id: "changed-oid".into(),
            email: EmailAddress::parse("User@Example.com").unwrap(),
            display_name: "Graph User".into(),
            aliases: vec![],
        };
        let reused = find_graph_account(&mut conn, &same_email)
            .await
            .unwrap()
            .expect("same email must reuse local account");
        assert_eq!(reused.id, existing.id);

        let different_email = RemoteAccount {
            account_id: "provider-only".into(),
            email: EmailAddress::parse("other@example.com").unwrap(),
            display_name: "Other".into(),
            aliases: vec![],
        };
        assert!(
            find_graph_account(&mut conn, &different_email)
                .await
                .unwrap()
                .is_none(),
            "different emails must not merge by Graph oid / provider metadata"
        );
    }

    fn paths(root: &std::path::Path) -> AppPaths {
        AppPaths {
            config_dir: root.join("config"),
            local_data_dir: root.to_owned(),
            data_dir: root.join("data"),
            attachments_dir: root.join("attachments"),
            credentials_dir: root.join("credentials"),
            database_path: root.join("test.db"),
        }
    }

    async fn current(db: &SqlitePool, id: &AccountId) -> AccountMailTransport {
        mail_transports::require(&mut *db.acquire().await.unwrap(), id)
            .await
            .unwrap()
    }

    fn graph_auth_of(transport: &AccountMailTransport) -> &GraphAuthConfig {
        match &transport.receive {
            ReceiveConfig::Graph(auth) => auth,
            ReceiveConfig::Imap(_) => panic!("expected graph receive config"),
        }
    }

    #[tokio::test]
    #[ignore = "writes an isolated temporary OS keyring entry"]
    async fn native_graph_sign_in_stages_tokens_and_switches_protocol_atomically() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        let cleanup_dir = paths.credentials_dir.clone();
        let db = db::open_memory().await.unwrap();
        let blocked_path = root.path().join("blocked");
        let result = tokio::spawn(async move {
            // 凭据存储失败时既不创建账号，也不提前修改登录态。
            let mut broken = paths.clone(); broken.credentials_dir = blocked_path;
            std::fs::write(&broken.credentials_dir, b"not a directory").unwrap();
            assert!(complete_graph_sign_in(&db, &broken, pending("first"), None).await.is_err());
            assert_eq!(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM accounts").fetch_one(&db).await.unwrap(), 0);

            // 新引用已保存后 INSERT 失败，暂存令牌回收，账号仍不存在。
            sqlx::query("CREATE TRIGGER fail_new_graph BEFORE INSERT ON accounts BEGIN SELECT RAISE(ABORT, 'injected'); END").execute(&db).await.unwrap();
            assert!(complete_graph_sign_in(&db, &paths, pending("first"), None).await.is_err());
            sqlx::query("DROP TRIGGER fail_new_graph").execute(&db).await.unwrap();
            assert!(secret_store::list_ids(&paths.credentials_dir, "graph").unwrap().is_empty());
            let account = complete_graph_sign_in(&db, &paths, pending("first"), None).await.unwrap();
            let original = current(&db, &account.id).await;
            let original_ref = original.receive.credential_ref().clone();
            assert_eq!(original.receive.protocol(), ReceiveProtocol::Graph);
            assert_eq!(original.send.protocol(), SendProtocol::Graph);
            assert_ne!(original_ref.as_str(), account.id.as_str());
            let auth = crate::infrastructure::config::AzureAuthConfig { client_id: "test-client".into(), client_secret: None,
                redirect_uri: "http://localhost/".into(), tenant: "tenant".into() };
            assert_eq!(
                graph_auth::get_access_token(&paths.credentials_dir, &auth, graph_auth_of(&original))
                    .await
                    .unwrap(),
                "first"
            );
            assert!(
                graph_auth::get_access_token(
                    &paths.credentials_dir,
                    &auth,
                    &GraphAuthConfig {
                        credential_ref: CredentialRef::new(account.id.as_str()).unwrap(),
                    }
                )
                .await
                .is_err()
            );

            sqlx::query("UPDATE accounts SET aliases_json='[\"alias@example.com\"]',is_signed_in=0 WHERE id=?").bind(account.id.as_str()).execute(&db).await.unwrap();
            sqlx::query("INSERT INTO mail_folder_sync_state(account_id,folder_id,protocol,remote_binding,display_name,sync_cursor,updated_at) VALUES (?,'graph-inbox','graph','remote-inbox','Inbox','keep-cursor',1)")
                .bind(account.id.as_str()).execute(&db).await.unwrap();
            // 更新和 COMMIT 两种失败都必须保留旧引用、退出态及原目录身份。
            for commit_failure in [false, true] {
                if commit_failure {
                    sqlx::query("CREATE TABLE graph_commit_guard (missing TEXT REFERENCES accounts(id) DEFERRABLE INITIALLY DEFERRED)").execute(&db).await.unwrap();
                    sqlx::query("CREATE TRIGGER fail_graph AFTER UPDATE ON account_mail_transport BEGIN INSERT INTO graph_commit_guard VALUES ('missing'); END").execute(&db).await.unwrap();
                } else {
                    sqlx::query("CREATE TRIGGER fail_graph BEFORE UPDATE ON account_mail_transport BEGIN SELECT RAISE(ABORT, 'injected'); END").execute(&db).await.unwrap();
                }
                assert!(complete_graph_sign_in(&db, &paths, pending("second"), Some(&account.id)).await.is_err());
                sqlx::query("DROP TRIGGER fail_graph").execute(&db).await.unwrap();
                assert_eq!(current(&db, &account.id).await.receive.credential_ref(), &original_ref);
                assert!(!accounts::is_signed_in(&mut *db.acquire().await.unwrap(), &account.id).await.unwrap());
                assert_eq!(
                    secret_store::list_ids(&paths.credentials_dir, "graph").unwrap(),
                    vec![original_ref.as_str().to_string()]
                );
            }
            assert!(complete_graph_sign_in(&db, &paths, pending("second"), None).await.is_err(), "预期账号不匹配时必须重试屏障");
            let mut metadata_changed = pending("second");
            metadata_changed.token_cache.home_account_id = Some("changed-provider-metadata".into());
            let renewed = complete_graph_sign_in(&db, &paths, metadata_changed, Some(&account.id)).await.unwrap();
            assert_eq!(renewed.id, account.id); assert_eq!(renewed.aliases[0].as_str(), "alias@example.com");
            let renewed_transport = current(&db, &account.id).await;
            let renewed_ref = renewed_transport.receive.credential_ref().clone();
            assert_ne!(renewed_ref, original_ref);
            assert_eq!(
                secret_store::list_ids(&paths.credentials_dir, "graph").unwrap(),
                vec![renewed_ref.as_str().to_string()]
            );
            let folder = sqlx::query_as::<_, (bool, String)>("SELECT is_retired,sync_cursor FROM mail_folder_sync_state WHERE account_id=? AND folder_id='graph-inbox'")
                .bind(account.id.as_str()).fetch_one(&db).await.unwrap();
            assert_eq!(folder, (false, "keep-cursor".into()));

            // Graph -> IMAP -> Graph 复用本地账号，清理正确命名空间并退役旧收信位置。
            let cmd = UpsertImapSmtpAccountCommand {
                email: "user@example.com".into(),
                display_name: "Mail".into(),
                imap_host: "imap.example.com".into(),
                imap_port: 993,
                imap_tls: MailTlsMode::Implicit,
                smtp_host: "smtp.example.com".into(),
                smtp_port: 587,
                smtp_tls: MailTlsMode::StartTls,
                imap_auth: ImapAuthenticationInput::Password {
                    username: "mail-user".into(),
                    password: " password ".into(),
                },
                smtp_auth: SmtpAuthenticationInput::Shared,
            };
            let generic = mail_account_use_case::upsert_imap_smtp_account(&db, &paths, cmd).await.unwrap();
            assert_eq!(generic.id, account.id); assert!(secret_store::list_ids(&paths.credentials_dir, "graph").unwrap().is_empty());
            sqlx::query("INSERT INTO mail_folder_sync_state(account_id,folder_id,protocol,remote_binding,display_name,updated_at) VALUES (?,'imap-inbox','imap','INBOX','Inbox',1)")
                .bind(account.id.as_str()).execute(&db).await.unwrap();
            sqlx::query("INSERT INTO sessions(id,account_id,kind,normalized_address,created_at,updated_at) VALUES ('test-session',?,'private','peer@example.com',1,1)").bind(account.id.as_str()).execute(&db).await.unwrap();
            sqlx::query("INSERT INTO messages(id,account_id,session_id,remote_key,location_status,location_revision,folder_id,body_content,local_updated_at) VALUES ('test-message',?,'test-session','imap:1:1','present',1,'imap-inbox','keep body',1)")
                .bind(account.id.as_str()).execute(&db).await.unwrap();
            let message_id = crate::domain::value_objects::MessageId::new("test-message");
            crate::infrastructure::repository::mutations::put_read(&mut *db.acquire().await.unwrap(), &account.id, &message_id, true).await.unwrap();
            let generic_transport = current(&db, &account.id).await;
            sqlx::query("CREATE TRIGGER fail_switch BEFORE UPDATE ON account_mail_transport BEGIN SELECT RAISE(ABORT, 'injected'); END").execute(&db).await.unwrap();
            assert!(complete_graph_sign_in(&db, &paths, pending("third"), Some(&account.id)).await.is_err());
            sqlx::query("DROP TRIGGER fail_switch").execute(&db).await.unwrap();
            assert_eq!(current(&db, &account.id).await, generic_transport);
            assert!(!sqlx::query_scalar::<_, bool>("SELECT is_retired FROM mail_folder_sync_state WHERE folder_id='imap-inbox'").fetch_one(&db).await.unwrap());
            let ReceiveConfig::Imap(generic_imap) = &generic_transport.receive else {
                panic!("expected imap receive config");
            };
            assert!(credentials::load(&paths.credentials_dir, &generic_imap.auth).await.unwrap().is_some());
            let switched = complete_graph_sign_in(&db, &paths, pending("third"), Some(&account.id)).await.unwrap();
            assert_eq!(switched.id, account.id);
            assert!(secret_store::list_ids(&paths.credentials_dir, "mail").unwrap().is_empty());
            assert!(sqlx::query_scalar::<_, bool>("SELECT is_retired FROM mail_folder_sync_state WHERE folder_id='imap-inbox'").fetch_one(&db).await.unwrap());
            let retained = sqlx::query_as::<_, (String,String)>("SELECT location_status,body_content FROM messages WHERE id='test-message'").fetch_one(&db).await.unwrap();
            assert_eq!(retained, ("invalidated".into(), "keep body".into()));
            assert!(!crate::infrastructure::repository::mutations::get(&mut *db.acquire().await.unwrap(), &account.id, &message_id).await.unwrap().unwrap().is_active);

            // 两种命名空间的崩溃孤儿均可清理，当前 Graph 引用保持有效。
            secret_store::save(&paths.credentials_dir, "graph", "orphan-graph", &pending("orphan").token_cache).unwrap();
            secret_store::save(&paths.credentials_dir, "mail", "orphan-mail", &"unused").unwrap();
            mail_account_use_case::recover_credentials(&db, &paths).await.unwrap();
            let final_transport = current(&db, &account.id).await;
            let final_ref = final_transport.receive.credential_ref().clone();
            assert_eq!(
                secret_store::list_ids(&paths.credentials_dir, "graph").unwrap(),
                vec![final_ref.as_str().to_string()]
            );
            assert!(secret_store::list_ids(&paths.credentials_dir, "mail").unwrap().is_empty());
            assert_eq!(
                graph_auth::get_access_token(&paths.credentials_dir, &auth, graph_auth_of(&final_transport))
                    .await
                    .unwrap(),
                "third"
            );
            // 退出清理失败可见；重试按配置引用清理，不能按 AccountId 猜文件。
            let file = std::fs::read_dir(&paths.credentials_dir).unwrap().map(|entry| entry.unwrap().path())
                .find(|path| path.extension().is_some_and(|extension| extension == "secret")).unwrap();
            std::fs::remove_file(&file).unwrap(); std::fs::create_dir(&file).unwrap();
            assert!(sign_out(&db, &paths, &account.id).await.is_err());
            assert!(accounts::is_signed_in(&mut *db.acquire().await.unwrap(), &account.id).await.unwrap());
            std::fs::remove_dir(&file).unwrap(); graph_auth::save_token_cache(&paths.credentials_dir, &final_ref, &pending("third").token_cache).await.unwrap();
            sign_out(&db, &paths, &account.id).await.unwrap();
            assert!(!accounts::is_signed_in(&mut *db.acquire().await.unwrap(), &account.id).await.unwrap());
            assert!(secret_store::list_ids(&paths.credentials_dir, "graph").unwrap().is_empty());
        }).await;
        let canonical = std::fs::canonicalize(&cleanup_dir).unwrap();
        let cleanup = keyring::Entry::new("com.postlet.credentials", &canonical.to_string_lossy())
            .unwrap()
            .delete_credential();
        result.unwrap();
        cleanup.unwrap();
    }
}
