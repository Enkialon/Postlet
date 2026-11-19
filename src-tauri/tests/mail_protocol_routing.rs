//! 协议路由与 IMAP 游标编解码回归。

use postlet_lib::domain::entities::{
    Account, AccountMailTransport, MailEndpoint, PasswordAuthConfig, PasswordMailConfig,
    ReceiveConfig, SendConfig,
};
use postlet_lib::domain::value_objects::{
    AccountId, CredentialRef, EmailAddress, MailTlsMode, ReceiveProtocol, SendProtocol, SyncStatus,
};
use postlet_lib::infrastructure::db;
use postlet_lib::infrastructure::mail::cursor::{
    encode_last_uid, encode_uidvalidity, parse_cursor,
};
use postlet_lib::infrastructure::repository::{accounts, mail_transports};

fn password_mail(
    host: &str,
    port: u16,
    tls: MailTlsMode,
    username: &str,
    reference: &str,
) -> PasswordMailConfig {
    PasswordMailConfig {
        endpoint: MailEndpoint::new(host.into(), port, tls).unwrap(),
        auth: PasswordAuthConfig::new(username.into(), CredentialRef::new(reference).unwrap())
            .unwrap(),
    }
}

fn with_imap_receive(
    transport: &AccountMailTransport,
    host: &str,
    port: u16,
    tls: MailTlsMode,
    username: &str,
    reference: &str,
) -> AccountMailTransport {
    AccountMailTransport {
        account_id: transport.account_id.clone(),
        receive: ReceiveConfig::Imap(password_mail(host, port, tls, username, reference)),
        send: transport.send.clone(),
        imap_idle_supported: transport.imap_idle_supported,
        updated_at: transport.updated_at,
    }
}

#[test]
fn imap_cursor_roundtrip() {
    let uv = encode_uidvalidity(9);
    let uid = encode_last_uid(42);
    let c = parse_cursor(Some(&uv), Some(&uid)).unwrap();
    assert_eq!(c.uidvalidity, 9);
    assert_eq!(c.high_uid, 42);
    assert_eq!(c.low_uid, 0);
}

#[tokio::test]
async fn mail_transport_requires_explicit_configuration_and_can_switch_to_imap_smtp() {
    let db = db::open_memory().await.unwrap();
    let mut conn = db.acquire().await.unwrap();
    let now = 1_700_000_000_000_i64;
    let account = Account {
        id: AccountId::new("acc-proto"),
        email: EmailAddress::new_unchecked("user@example.com"),
        display_name: "User".into(),
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
    accounts::upsert(&mut conn, &account).await.unwrap();
    assert!(mail_transports::require(&mut conn, &account.id).await.is_err());
    assert!(mail_transports::get(&mut conn, &account.id).await.unwrap().is_none());
    assert!(sqlx::query("INSERT INTO account_mail_transport(account_id,updated_at) VALUES (?,?)")
        .bind(account.id.as_str()).bind(now).execute(&mut *conn).await.is_err());
    let graph = AccountMailTransport::graph(
        account.id.clone(),
        CredentialRef::new("graph-reference").unwrap(),
        now,
    );
    mail_transports::upsert(&mut conn, &graph).await.unwrap();
    let t = mail_transports::require(&mut conn, &account.id).await.unwrap();
    assert_eq!(t.receive.protocol(), ReceiveProtocol::Graph);
    assert_eq!(t.send.protocol(), SendProtocol::Graph);

    let reference = "test-credential";
    let imap = AccountMailTransport {
        account_id: account.id.clone(),
        receive: ReceiveConfig::Imap(password_mail(
            "imap.example.com",
            993,
            MailTlsMode::Implicit,
            "user@example.com",
            reference,
        )),
        send: SendConfig::Smtp(password_mail(
            "smtp.example.com",
            587,
            MailTlsMode::StartTls,
            "user@example.com",
            reference,
        )),
        imap_idle_supported: Some(true),
        updated_at: now,
    };
    mail_transports::upsert(&mut conn, &imap).await.unwrap();
    let loaded = mail_transports::get(&mut conn, &account.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.receive.protocol(), ReceiveProtocol::Imap);
    assert_eq!(loaded.send.protocol(), SendProtocol::Smtp);
    let ReceiveConfig::Imap(loaded_imap) = &loaded.receive else {
        panic!("expected imap");
    };
    assert_eq!(loaded_imap.endpoint.port(), 993);
    assert_eq!(loaded.imap_idle_supported, Some(true));

    // SMTP 配置可以独立改变；收信能力观测只更新自己的字段，不整行写回旧快照。
    let mut current = imap.clone();
    current.send = SendConfig::Smtp(password_mail(
        "new-smtp.example.com",
        587,
        MailTlsMode::StartTls,
        "user@example.com",
        reference,
    ));
    current.updated_at = now + 1;
    mail_transports::upsert(&mut conn, &current).await.unwrap();
    assert!(mail_transports::record_imap_idle_support(&mut conn, &imap, false).await.unwrap());
    let after = mail_transports::get(&mut conn, &account.id).await.unwrap().unwrap();
    let SendConfig::Smtp(after_smtp) = &after.send else {
        panic!("expected smtp");
    };
    assert_eq!(after_smtp.endpoint.host(), "new-smtp.example.com");
    assert_eq!(after.updated_at, current.updated_at);
    assert_eq!(after.imap_idle_supported, Some(false));

    // 挂起的旧探测在凭据轮换后返回，不能恢复旧引用或写入旧能力。
    current = with_imap_receive(
        &current,
        "imap.example.com",
        993,
        MailTlsMode::Implicit,
        "user@example.com",
        "rotated-credential",
    );
    current.imap_idle_supported = None;
    current.updated_at += 1;
    mail_transports::upsert(&mut conn, &current).await.unwrap();
    assert!(!mail_transports::record_imap_idle_support(&mut conn, &imap, true).await.unwrap());
    let after = mail_transports::get(&mut conn, &account.id).await.unwrap().unwrap();
    assert_eq!(
        after.receive.credential_ref(),
        current.receive.credential_ref()
    );
    assert_eq!(after.imap_idle_supported, None);
    assert_eq!(after.updated_at, current.updated_at);

    // 同一个引用下改变端点、TLS 或用户名，同样不能接受旧观测。
    for field in ["host", "port", "tls", "username", "protocol"] {
        let mut changed = match field {
            "host" => with_imap_receive(
                &current,
                "new-imap.example.com",
                993,
                MailTlsMode::Implicit,
                "user@example.com",
                "rotated-credential",
            ),
            "port" => with_imap_receive(
                &current,
                "imap.example.com",
                143,
                MailTlsMode::Implicit,
                "user@example.com",
                "rotated-credential",
            ),
            "tls" => with_imap_receive(
                &current,
                "imap.example.com",
                993,
                MailTlsMode::StartTls,
                "user@example.com",
                "rotated-credential",
            ),
            "username" => with_imap_receive(
                &current,
                "imap.example.com",
                993,
                MailTlsMode::Implicit,
                "other-user",
                "rotated-credential",
            ),
            "protocol" => AccountMailTransport::graph(
                current.account_id.clone(),
                CredentialRef::new("rotated-credential").unwrap(),
                current.updated_at,
            ),
            _ => unreachable!(),
        };
        changed.updated_at = current.updated_at;
        mail_transports::upsert(&mut conn, &changed).await.unwrap();
        assert!(
            !mail_transports::record_imap_idle_support(&mut conn, &current, true)
                .await
                .unwrap(),
            "{field}"
        );
        let after = mail_transports::get(&mut conn, &account.id).await.unwrap().unwrap();
        assert_eq!(after.imap_idle_supported, None);
    }

    mail_transports::upsert(&mut conn, &current).await.unwrap();
    accounts::set_signed_in(&mut conn, &account.id, false).await.unwrap();
    assert!(!mail_transports::record_imap_idle_support(&mut conn, &current, true).await.unwrap());
    assert_eq!(mail_transports::get(&mut conn, &account.id).await.unwrap().unwrap().imap_idle_supported, None);
}
