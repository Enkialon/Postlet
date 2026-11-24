use crate::domain::entities::*;
use crate::domain::services::{FolderRole, RoleSource};
use crate::domain::value_objects::*;
use sqlx::{Row, sqlite::SqliteRow};

fn parse_syncstatus(value: String) -> Result<SyncStatus, sqlx::Error> {
    match value.as_str() {
        "idle" => Ok(SyncStatus::Idle),
        "running" => Ok(SyncStatus::Running),
        "error" => Ok(SyncStatus::Error),
        _ => Err(sqlx::Error::Decode("无效领域枚举值".into())),
    }
}

fn parse_bodycontenttype(value: String) -> Result<BodyContentType, sqlx::Error> {
    match value.as_str() {
        "text" => Ok(BodyContentType::Text),
        "html" => Ok(BodyContentType::Html),
        _ => Err(sqlx::Error::Decode("无效领域枚举值".into())),
    }
}

fn parse_importance(value: String) -> Result<Importance, sqlx::Error> {
    match value.as_str() {
        "low" => Ok(Importance::Low),
        "normal" => Ok(Importance::Normal),
        "high" => Ok(Importance::High),
        _ => Err(sqlx::Error::Decode("无效领域枚举值".into())),
    }
}

fn parse_recipientrole(value: String) -> Result<RecipientRole, sqlx::Error> {
    match value.as_str() {
        "to" => Ok(RecipientRole::To),
        "cc" => Ok(RecipientRole::Cc),
        "bcc" => Ok(RecipientRole::Bcc),
        "reply_to" => Ok(RecipientRole::ReplyTo),
        _ => Err(sqlx::Error::Decode("无效领域枚举值".into())),
    }
}

fn parse_groupsource(value: String) -> Result<GroupSource, sqlx::Error> {
    match value.as_str() {
        "auto" => Ok(GroupSource::Auto),
        "manual" => Ok(GroupSource::Manual),
        _ => Err(sqlx::Error::Decode("无效领域枚举值".into())),
    }
}

fn parse_contactsource(value: String) -> Result<ContactSource, sqlx::Error> {
    match value.as_str() {
        "graph" => Ok(ContactSource::Graph),
        "manual" => Ok(ContactSource::Manual),
        "observed" => Ok(ContactSource::Observed),
        _ => Err(sqlx::Error::Decode("无效领域枚举值".into())),
    }
}

fn parse_sessionkind(value: String) -> Result<SessionKind, sqlx::Error> {
    match value.as_str() {
        "private" => Ok(SessionKind::Private),
        "group" => Ok(SessionKind::Group),
        _ => Err(sqlx::Error::Decode("无效领域枚举值".into())),
    }
}

pub(super) fn account(row: &SqliteRow) -> Result<Account, sqlx::Error> {
    Ok(Account {
        id: AccountId::new(row.try_get::<String, _>("id")?),
        email: EmailAddress::new_unchecked(row.try_get::<String, _>("email")?),
        display_name: row.try_get("display_name")?,
        aliases: serde_json::from_str::<Vec<String>>(&row.try_get::<String, _>("aliases_json")?)
            .map_err(|e| sqlx::Error::Decode(Box::new(e)))?
            .into_iter()
            .map(EmailAddress::new_unchecked)
            .collect(),
        contact_delta_link: row.try_get("contact_delta_link")?,
        mail_sync_triggered_at: row.try_get("mail_sync_triggered_at")?,
        contact_synced_at: row.try_get("contact_synced_at")?,
        mail_sync_status: parse_syncstatus(row.try_get("mail_sync_status")?)?,
        mail_sync_error: row.try_get("mail_sync_error")?,
        include_in_all: row.try_get("include_in_all")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

pub(super) fn contact(row: &SqliteRow) -> Result<Contact, sqlx::Error> {
    Ok(Contact {
        id: ContactId::new(row.try_get::<String, _>("id")?),
        account_id: AccountId::new(row.try_get::<String, _>("account_id")?),
        graph_id: row.try_get("graph_id")?,
        display_name: row.try_get("display_name")?,
        normalized_address: EmailAddress::new_unchecked(
            row.try_get::<String, _>("normalized_address")?,
        ),
        source: parse_contactsource(row.try_get("source")?)?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

pub(super) fn session(row: &SqliteRow) -> Result<Session, sqlx::Error> {
    Ok(Session {
        id: SessionId::new(row.try_get::<String, _>("id")?),
        account_id: AccountId::new(row.try_get::<String, _>("account_id")?),
        kind: parse_sessionkind(row.try_get("kind")?)?,
        normalized_address: row
            .try_get::<Option<String>, _>("normalized_address")?
            .map(EmailAddress::new_unchecked),
        contact_id: row
            .try_get::<Option<String>, _>("contact_id")?
            .map(ContactId::new),
        group_id: row
            .try_get::<Option<String>, _>("group_id")?
            .map(GroupId::new),
        merged_into_id: row
            .try_get::<Option<String>, _>("merged_into_id")?
            .map(SessionId::new),
        last_message_at: row.try_get("last_message_at")?,
        last_message_id: row
            .try_get::<Option<String>, _>("last_message_id")?
            .map(MessageId::new),
        unread_count: row.try_get("unread_count")?,
        hidden_at: row.try_get("hidden_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

pub(super) fn group(row: &SqliteRow) -> Result<Group, sqlx::Error> {
    Ok(Group {
        id: GroupId::new(row.try_get::<String, _>("id")?),
        account_id: AccountId::new(row.try_get::<String, _>("account_id")?),
        name: row.try_get("name")?,
        name_is_manual: row.try_get("name_is_manual")?,
        source: parse_groupsource(row.try_get("source")?)?,
        send_members_is_manual: row.try_get("send_members_is_manual")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

pub(super) fn groupparticipant(row: &SqliteRow) -> Result<GroupParticipant, sqlx::Error> {
    Ok(GroupParticipant {
        account_id: AccountId::new(row.try_get::<String, _>("account_id")?),
        group_id: GroupId::new(row.try_get::<String, _>("group_id")?),
        normalized_address: EmailAddress::new_unchecked(
            row.try_get::<String, _>("normalized_address")?,
        ),
        display_name: row.try_get("display_name")?,
        is_observed: row.try_get("is_observed")?,
        is_send_member: row.try_get("is_send_member")?,
        first_seen_at: row.try_get("first_seen_at")?,
        last_seen_at: row.try_get("last_seen_at")?,
    })
}

pub(super) fn message(row: &SqliteRow) -> Result<Message, sqlx::Error> {
    Ok(Message {
        id: MessageId::new(row.try_get::<String, _>("id")?),
        account_id: AccountId::new(row.try_get::<String, _>("account_id")?),
        session_id: SessionId::new(row.try_get::<String, _>("session_id")?),
        remote_key: row
            .try_get::<Option<String>, _>("remote_key")?
            .map(RemoteKey::new),
        location_status: row.try_get::<Option<String>, _>("location_status")?.map(|value| crate::domain::mail::LocationStatus::parse(&value).ok_or_else(|| sqlx::Error::Decode("无效定位状态".into()))).transpose()?,
        location_revision: row.try_get("location_revision")?,
        internet_message_id: row.try_get("internet_message_id")?,
        in_reply_to: row.try_get("in_reply_to")?,
        subject: row.try_get("subject")?,
        body_preview: row.try_get("body_preview")?,
        body_content_type: parse_bodycontenttype(row.try_get("body_content_type")?)?,
        body_content: row.try_get("body_content")?,
        from_address: row
            .try_get::<Option<String>, _>("from_address")?
            .map(EmailAddress::new_unchecked),
        from_name: row.try_get("from_name")?,
        received_at: row.try_get("received_at")?,
        sent_at: row.try_get("sent_at")?,
        is_read: row.try_get("is_read")?,
        is_draft: row.try_get("is_draft")?,
        remote_is_read: row.try_get("remote_is_read")?,
        remote_is_draft: row.try_get("remote_is_draft")?,
        remote_is_deleted: row.try_get("remote_is_deleted")?,
        is_outbound: row.try_get("is_outbound")?,
        has_attachments: row.try_get("has_attachments")?,
        importance: parse_importance(row.try_get("importance")?)?,
        folder_id: row
            .try_get::<Option<String>, _>("folder_id")?
            .map(FolderId::new),
        parent_folder_name: row.try_get("parent_folder_name")?,
        local_updated_at: row.try_get("local_updated_at")?,
        remote_updated_at: row.try_get("remote_updated_at")?,
    })
}

pub(super) fn messagerecipient(row: &SqliteRow) -> Result<MessageRecipient, sqlx::Error> {
    Ok(MessageRecipient {
        account_id: AccountId::new(row.try_get::<String, _>("account_id")?),
        message_id: MessageId::new(row.try_get::<String, _>("message_id")?),
        role: parse_recipientrole(row.try_get("role")?)?,
        seq: row.try_get("seq")?,
        normalized_address: EmailAddress::new_unchecked(
            row.try_get::<String, _>("normalized_address")?,
        ),
        display_name: row.try_get("display_name")?,
    })
}

pub(super) fn mailfoldersyncstate(row: &SqliteRow) -> Result<MailFolderSyncState, sqlx::Error> {
    let parent = row.try_get::<Option<String>, _>("parent_folder_id")?;
    let sync_enabled: i64 = row.try_get("sync_enabled").unwrap_or(0);
    let role = FolderRole::parse(&row.try_get::<String, _>("role").unwrap_or_default())
        .unwrap_or(FolderRole::Custom);
    let junk_override = row
        .try_get::<Option<i64>, _>("junk_override")?
        .map(|value| value != 0);
    Ok(MailFolderSyncState {
        account_id: AccountId::new(row.try_get::<String, _>("account_id")?),
        folder_id: FolderId::new(row.try_get::<String, _>("folder_id")?),
        protocol: ReceiveProtocol::parse(&row.try_get::<String, _>("protocol")?).ok_or_else(|| sqlx::Error::Decode("无效文件夹协议".into()))?,
        remote_binding: row.try_get("remote_binding")?,
        binding_revision: row.try_get("binding_revision")?,
        is_retired: row.try_get("is_retired")?,
        parent_folder_id: parent.filter(|s| !s.is_empty()).map(FolderId::new),
        display_name: row.try_get::<String, _>("display_name").unwrap_or_default(),
        role,
        role_source: RoleSource::parse(
            &row.try_get::<String, _>("role_source").unwrap_or_default(),
        )
        .unwrap_or(RoleSource::Unknown),
        junk_override,
        effective_role: crate::domain::services::effective_role(role, junk_override),
        sync_enabled: sync_enabled != 0,
        watch_enabled: row.try_get::<Option<bool>, _>("watch_override")?.unwrap_or(role == FolderRole::Inbox),
        generation: row.try_get("generation")?,
        sync_cursor: row.try_get("sync_cursor")?,
        page_cursor: row.try_get("page_cursor")?,
        synced_at: row.try_get("synced_at")?,
        sync_status: parse_syncstatus(row.try_get("sync_status")?)?,
        last_error: row.try_get("last_error")?,
        updated_at: row.try_get("updated_at")?,
    })
}
