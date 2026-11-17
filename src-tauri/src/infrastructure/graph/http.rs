//! Graph 绝对 URL 请求（含 `$skiptoken` / `$select` 等 query）。
//!
//! `graph-rs-sdk` 的 `custom_endpoint` / `use_endpoint` **禁止带 query**，
//! 对 OData nextLink / deltaLink 会直接 panic；这里用 reqwest 直发。

use crate::infrastructure::error::InfraError;
use url::Url;

pub async fn get_absolute(
    access_token: &str,
    link: &str,
    context: &str,
) -> Result<reqwest::Response, InfraError> {
    let url = Url::parse(link).map_err(|_| InfraError::Remote {
        message: format!("无效 Graph 链接: {link}"),
        retryable: false,
    })?;
    let response = reqwest::Client::new()
        .get(url)
        .bearer_auth(access_token)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|e| InfraError::Network {
            message: format!("{context}: {e}"),
        })?;
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let retryable = status.as_u16() == 429 || status.is_server_error();
    let body = response.text().await.unwrap_or_default();
    Err(InfraError::Remote {
        message: format!("{context} 失败 ({status}): {body}"),
        retryable,
    })
}
