//! SQLite conversations 模块函数；连接与事务由调用方持有。

use crate::application::dto::{
    ConversationListItem, ConversationListQuery, ConversationMessagesQuery, CursorPage,
};
use crate::application::error::PortError;
use crate::domain::entities::{Conversation, ConversationMessageLink, MessageWithRecipients};
use crate::domain::value_objects::{
    AccountId, ContactId, ConversationId, EmailAddress, GroupId, MessageId,
};
use sqlx::SqliteConnection;

pub async fn get(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    id: &ConversationId,
) -> Result<Option<Conversation>, PortError> {
    let row = sqlx::query("SELECT * FROM conversations WHERE account_id = ? AND id = ?")
        .bind(account_id.as_str())
        .bind(id.as_str())
        .fetch_optional(&mut *conn)
        .await?;
    row.as_ref()
        .map(super::rows::conversation)
        .transpose()
        .map_err(Into::into)
}

pub async fn get_contact_conversation(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    address: &EmailAddress,
) -> Result<Option<Conversation>, PortError> {
    let _ = (conn, account_id, address);
    Err(PortError::Storage {
        message: "尚未实现: repository::conversations::get_contact_conversation".into(),
    })
}

pub async fn get_group_conversation(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    group_id: &GroupId,
) -> Result<Option<Conversation>, PortError> {
    let _ = (conn, account_id, group_id);
    Err(PortError::Storage {
        message: "尚未实现: repository::conversations::get_group_conversation".into(),
    })
}

pub async fn list(
    conn: &mut SqliteConnection,
    query: &ConversationListQuery,
) -> Result<CursorPage<ConversationListItem>, PortError> {
    let kind = query.kind.map(|k| match k {
        crate::domain::value_objects::ConversationKind::Contact => "contact",
        crate::domain::value_objects::ConversationKind::Group => "group",
    });
    if query.limit == 0 || query.limit > 500 {
        return Err(PortError::Conflict {
            message: "分页大小须为 1..500".into(),
        });
    }
    let rows = sqlx::query("SELECT * FROM conversations WHERE account_id = ? AND (? IS NULL OR id > ?) AND (? IS NULL OR kind = ?) ORDER BY id LIMIT ?")
        .bind(query.account_id.as_str()).bind(query.cursor.as_deref()).bind(query.cursor.as_deref()).bind(kind).bind(kind).bind(i64::from(query.limit) + 1).fetch_all(&mut *conn).await?;
    let has_more = rows.len() > query.limit as usize;
    let mut items = rows
        .iter()
        .take(query.limit as usize)
        .map(super::rows::conversation)
        .collect::<Result<Vec<_>, _>>()?;
    let next_cursor = if has_more {
        items.last().map(|x| x.id.as_str().to_owned())
    } else {
        None
    };
    let items = items
        .drain(..)
        .map(|conversation| ConversationListItem {
            conversation,
            last_message_preview: None,
            contact_display_name: None,
            group_name: None,
        })
        .collect();
    Ok(CursorPage {
        items,
        next_cursor,
        has_more,
    })
}

pub async fn upsert(
    conn: &mut SqliteConnection,
    conversation: &Conversation,
) -> Result<(), PortError> {
    let _ = (conn, conversation);
    Err(PortError::Storage {
        message: "尚未实现: repository::conversations::upsert".into(),
    })
}

pub async fn clear_contact_id_refs(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    contact_id: &ContactId,
) -> Result<(), PortError> {
    let _ = (conn, account_id, contact_id);
    Err(PortError::Storage {
        message: "尚未实现: repository::conversations::clear_contact_id_refs".into(),
    })
}

pub async fn attach_messages(
    conn: &mut SqliteConnection,
    links: &[ConversationMessageLink],
) -> Result<(), PortError> {
    let _ = (conn, links);
    Err(PortError::Storage {
        message: "尚未实现: repository::conversations::attach_messages".into(),
    })
}

pub async fn detach_messages(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    conversation_id: &ConversationId,
    message_ids: &[MessageId],
) -> Result<(), PortError> {
    let _ = (conn, account_id, conversation_id, message_ids);
    Err(PortError::Storage {
        message: "尚未实现: repository::conversations::detach_messages".into(),
    })
}

pub async fn list_message_ids(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    conversation_id: &ConversationId,
) -> Result<Vec<MessageId>, PortError> {
    let _ = (conn, account_id, conversation_id);
    Err(PortError::Storage {
        message: "尚未实现: repository::conversations::list_message_ids".into(),
    })
}

pub async fn list_messages(
    conn: &mut SqliteConnection,
    query: &ConversationMessagesQuery,
) -> Result<CursorPage<MessageWithRecipients>, PortError> {
    let _ = (conn, query);
    Err(PortError::Storage {
        message: "尚未实现: repository::conversations::list_messages".into(),
    })
}

pub async fn recompute_projection(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    conversation_id: &ConversationId,
) -> Result<Conversation, PortError> {
    let _ = (conn, account_id, conversation_id);
    Err(PortError::Storage {
        message: "尚未实现: repository::conversations::recompute_projection".into(),
    })
}

pub async fn find_conversation_ids_by_message(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Vec<ConversationId>, PortError> {
    let _ = (conn, account_id, message_id);
    Err(PortError::Storage {
        message: "尚未实现: repository::conversations::find_conversation_ids_by_message".into(),
    })
}
