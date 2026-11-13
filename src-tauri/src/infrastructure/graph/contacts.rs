//! Graph/OAuth 模块骨架；后续仅在此处使用 graph-rs-sdk。

use crate::application::dto::remote_models::ContactDeltaPage;
use crate::application::error::PortError;
use crate::domain::value_objects::AccountId;

pub async fn delta_contacts(
    account_id: &AccountId,
    delta_or_next_link: Option<&str>,
) -> Result<ContactDeltaPage, PortError> {
    let _ = (account_id, delta_or_next_link);
    Err(PortError::Remote {
        message: "尚未实现: graph::contacts::delta_contacts".into(),
        retryable: false,
    })
}
