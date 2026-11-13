use crate::domain::entities::*;
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

fn parse_messagesyncstatus(value: String) -> Result<MessageSyncStatus, sqlx::Error> {
    match value.as_str() {
        "synced" => Ok(MessageSyncStatus::Synced),
        "pending_local" => Ok(MessageSyncStatus::PendingLocal),
        "deleted_remote" => Ok(MessageSyncStatus::DeletedRemote),
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

fn parse_conversationkind(value: String) -> Result<ConversationKind, sqlx::Error> {
    match value.as_str() {
        "contact" => Ok(ConversationKind::Contact),
        "group" => Ok(ConversationKind::Group),
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
        tenant_id: row.try_get("tenant_id")?,
        home_account_id: row.try_get("home_account_id")?,
        contact_delta_link: row.try_get("contact_delta_link")?,
        mail_synced_at: row.try_get("mail_synced_at")?,
        contact_synced_at: row.try_get("contact_synced_at")?,
        mail_sync_status: parse_syncstatus(row.try_get("mail_sync_status")?)?,
        mail_sync_error: row.try_get("mail_sync_error")?,
        show_group_messages_in_contact_conversation: row
            .try_get("show_group_messages_in_contact_conversation")?,
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
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

pub(super) fn conversation(row: &SqliteRow) -> Result<Conversation, sqlx::Error> {
    Ok(Conversation {
        id: ConversationId::new(row.try_get::<String, _>("id")?),
        account_id: AccountId::new(row.try_get::<String, _>("account_id")?),
        kind: parse_conversationkind(row.try_get("kind")?)?,
        normalized_address: row
            .try_get::<Option<String>, _>("normalized_address")?
            .map(EmailAddress::new_unchecked),
        contact_id: row
            .try_get::<Option<String>, _>("contact_id")?
            .map(ContactId::new),
        group_id: row
            .try_get::<Option<String>, _>("group_id")?
            .map(GroupId::new),
        last_message_at: row.try_get("last_message_at")?,
        last_message_id: row
            .try_get::<Option<String>, _>("last_message_id")?
            .map(MessageId::new),
        unread_count: row.try_get("unread_count")?,
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
        merged_into_id: row
            .try_get::<Option<String>, _>("merged_into_id")?
            .map(GroupId::new),
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
        graph_id: row
            .try_get::<Option<String>, _>("graph_id")?
            .map(GraphImmutableId::new),
        thread_id: row
            .try_get::<Option<String>, _>("thread_id")?
            .map(ThreadId::new),
        graph_change_key: row.try_get("graph_change_key")?,
        internet_message_id: row.try_get("internet_message_id")?,
        conversation_index: row.try_get("conversation_index")?,
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
        created_at_graph: row.try_get("created_at_graph")?,
        is_read: row.try_get("is_read")?,
        is_draft: row.try_get("is_draft")?,
        is_outbound: row.try_get("is_outbound")?,
        has_attachments: row.try_get("has_attachments")?,
        importance: parse_importance(row.try_get("importance")?)?,
        folder_id: row
            .try_get::<Option<String>, _>("folder_id")?
            .map(FolderId::new),
        parent_folder_name: row.try_get("parent_folder_name")?,
        sync_status: parse_messagesyncstatus(row.try_get("sync_status")?)?,
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
    Ok(MailFolderSyncState {
        account_id: AccountId::new(row.try_get::<String, _>("account_id")?),
        folder_id: FolderId::new(row.try_get::<String, _>("folder_id")?),
        delta_link: row.try_get("delta_link")?,
        next_link: row.try_get("next_link")?,
        synced_at: row.try_get("synced_at")?,
        sync_status: parse_syncstatus(row.try_get("sync_status")?)?,
        last_error: row.try_get("last_error")?,
        updated_at: row.try_get("updated_at")?,
    })
}
