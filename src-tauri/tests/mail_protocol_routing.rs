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
    assert_eq!(c.last_uid, 42);
}

#[tokio::test]
async fn mail_transport_defaults_to_graph_and_can_switch_to_imap_smtp() {
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
        show_group_messages_in_contact_conversation: false,
        include_in_all: true,
        created_at: now,
        updated_at: now,
    };
    accounts::upsert(&mut conn, &account).await.unwrap();
    let t = mail_transports::ensure_default(&mut conn, &account.id, now)
        .await
        .unwrap();
    assert_eq!(t.receive_protocol, ReceiveProtocol::Graph);
    assert_eq!(t.send_protocol, SendProtocol::Graph);

    let imap = AccountMailTransport {
        account_id: account.id.clone(),
        receive_protocol: ReceiveProtocol::Imap,
        send_protocol: SendProtocol::Smtp,
        auth_kind: MailAuthKind::Password,
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
}
