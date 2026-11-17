//! 协议路由与 IMAP 游标编解码回归。

use postlet_lib::domain::entities::AccountMailTransport;
use postlet_lib::domain::value_objects::{
    AccountId, MailAuthKind, MailTlsMode, ReceiveProtocol, SendProtocol,
};
use postlet_lib::infrastructure::db;
use postlet_lib::infrastructure::mail::cursor::{
    encode_last_uid, encode_uidvalidity, parse_cursor,
};
use postlet_lib::infrastructure::repository::{accounts, mail_transports};
use postlet_lib::domain::entities::Account;
use postlet_lib::domain::value_objects::{EmailAddress, SyncStatus};

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
        tenant_id: None,
        home_account_id: None,
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
    let graph = AccountMailTransport::graph_default(account.id.clone(), "graph-reference".into(), now);
    mail_transports::upsert(&mut conn, &graph).await.unwrap();
    let t = mail_transports::require(&mut conn, &account.id).await.unwrap();
    assert_eq!(t.receive_protocol, ReceiveProtocol::Graph);
    assert_eq!(t.send_protocol, SendProtocol::Graph);

    let imap = AccountMailTransport {
        account_id: account.id.clone(),
        receive_protocol: ReceiveProtocol::Imap,
        send_protocol: SendProtocol::Smtp,
        auth_kind: MailAuthKind::Password,
        credential_ref: Some("test-credential".into()),
        imap_host: Some("imap.example.com".into()),
        imap_port: Some(993),
        imap_tls: Some(MailTlsMode::Implicit),
        smtp_host: Some("smtp.example.com".into()),
        smtp_port: Some(587),
        smtp_tls: Some(MailTlsMode::StartTls),
        username: Some("user@example.com".into()),
        imap_idle_supported: Some(true),
        updated_at: now,
    };
    mail_transports::upsert(&mut conn, &imap).await.unwrap();
    let loaded = mail_transports::get(&mut conn, &account.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.receive_protocol, ReceiveProtocol::Imap);
    assert_eq!(loaded.send_protocol, SendProtocol::Smtp);
    assert_eq!(loaded.imap_port, Some(993));
    assert_eq!(loaded.imap_idle_supported, Some(true));

    // SMTP 配置可以独立改变；收信能力观测只更新自己的字段，不整行写回旧快照。
    let mut current = imap.clone();
    current.smtp_host = Some("new-smtp.example.com".into());
    current.updated_at = now + 1;
    mail_transports::upsert(&mut conn, &current).await.unwrap();
    assert!(mail_transports::record_imap_idle_support(&mut conn, &imap, false).await.unwrap());
    let after = mail_transports::get(&mut conn, &account.id).await.unwrap().unwrap();
    assert_eq!(after.smtp_host, current.smtp_host);
    assert_eq!(after.updated_at, current.updated_at);
    assert_eq!(after.imap_idle_supported, Some(false));

    // 挂起的旧探测在凭据轮换后返回，不能恢复旧引用或写入旧能力。
    current.credential_ref = Some("rotated-credential".into());
    current.imap_idle_supported = None;
    current.updated_at += 1;
    mail_transports::upsert(&mut conn, &current).await.unwrap();
    assert!(!mail_transports::record_imap_idle_support(&mut conn, &imap, true).await.unwrap());
    let after = mail_transports::get(&mut conn, &account.id).await.unwrap().unwrap();
    assert_eq!(after.credential_ref, current.credential_ref);
    assert_eq!(after.imap_idle_supported, None);
    assert_eq!(after.updated_at, current.updated_at);

    // 同一个引用下改变端点、TLS 或用户名，同样不能接受旧观测。
    for field in ["host", "port", "tls", "username", "protocol"] {
        let mut changed = current.clone();
        match field {
            "host" => changed.imap_host = Some("new-imap.example.com".into()),
            "port" => changed.imap_port = Some(143),
            "tls" => changed.imap_tls = Some(MailTlsMode::StartTls),
            "username" => changed.username = Some("other-user".into()),
            "protocol" => changed.receive_protocol = ReceiveProtocol::Graph,
            _ => unreachable!(),
        }
        mail_transports::upsert(&mut conn, &changed).await.unwrap();
        assert!(!mail_transports::record_imap_idle_support(&mut conn, &current, true).await.unwrap(), "{field}");
        let after = mail_transports::get(&mut conn, &account.id).await.unwrap().unwrap();
        assert_eq!(after.imap_idle_supported, None);
    }

    mail_transports::upsert(&mut conn, &current).await.unwrap();
    accounts::set_signed_in(&mut conn, &account.id, false).await.unwrap();
    assert!(!mail_transports::record_imap_idle_support(&mut conn, &current, true).await.unwrap());
    assert_eq!(mail_transports::get(&mut conn, &account.id).await.unwrap().unwrap().imap_idle_supported, None);
}
