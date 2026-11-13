//! Graph/OAuth 模块骨架；后续仅在此处使用 graph-rs-sdk。

use crate::application::dto::remote_models::RemoteAccount;
use crate::application::error::PortError;
use crate::domain::value_objects::AccountId;

pub async fn list_signed_in_accounts() -> Result<Vec<RemoteAccount>, PortError> {
    let _ = ();
    Err(PortError::Remote {
        message: "尚未实现: graph::auth::list_signed_in_accounts".into(),
        retryable: false,
    })
}

pub async fn sign_in() -> Result<RemoteAccount, PortError> {
    let _ = ();
    Err(PortError::Remote {
        message: "尚未实现: graph::auth::sign_in".into(),
        retryable: false,
    })
}

pub async fn sign_out(account_id: &AccountId) -> Result<(), PortError> {
    let _ = account_id;
    Err(PortError::Remote {
        message: "尚未实现: graph::auth::sign_out".into(),
        retryable: false,
    })
}

pub async fn get_access_token(account_id: &AccountId) -> Result<String, PortError> {
    let _ = account_id;
    Err(PortError::Remote {
        message: "尚未实现: graph::auth::get_access_token".into(),
        retryable: false,
    })
}
