//! Graph/OAuth 模块骨架；后续仅在此处使用 graph-rs-sdk。

use crate::application::dto::remote_models::{MailDeltaPage, RemoteMessage, RemoteMessageLocation};
use crate::application::error::PortError;
use crate::domain::value_objects::{AccountId, BodyContentType, FolderId, GraphImmutableId};

pub async fn get_message_snapshot(
    account_id: &AccountId,
    graph_id: &GraphImmutableId,
) -> Result<Option<RemoteMessage>, PortError> {
    let _ = (account_id, graph_id);
    Err(PortError::Remote {
        message: "尚未实现: graph::mail::get_message_snapshot".into(),
        retryable: false,
    })
}

pub async fn delta_messages(
    account_id: &AccountId,
    folder_id: &FolderId,
    delta_or_next_link: Option<&str>,
) -> Result<MailDeltaPage, PortError> {
    let _ = (account_id, folder_id, delta_or_next_link);
    Err(PortError::Remote {
        message: "尚未实现: graph::mail::delta_messages".into(),
        retryable: false,
    })
}

pub async fn locate_message(
    account_id: &AccountId,
    graph_id: &GraphImmutableId,
) -> Result<RemoteMessageLocation, PortError> {
    let _ = (account_id, graph_id);
    Err(PortError::Remote {
        message: "尚未实现: graph::mail::locate_message".into(),
        retryable: false,
    })
}

pub async fn get_message_body(
    account_id: &AccountId,
    graph_id: &GraphImmutableId,
) -> Result<(BodyContentType, String), PortError> {
    let _ = (account_id, graph_id);
    Err(PortError::Remote {
        message: "尚未实现: graph::mail::get_message_body".into(),
        retryable: false,
    })
}

pub async fn patch_message_read(
    account_id: &AccountId,
    graph_id: &GraphImmutableId,
    is_read: bool,
) -> Result<(), PortError> {
    let _ = (account_id, graph_id, is_read);
    Err(PortError::Remote {
        message: "尚未实现: graph::mail::patch_message_read".into(),
        retryable: false,
    })
}
